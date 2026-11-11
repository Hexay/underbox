//! Auto exposure for an `ITonemapSystem` (parameters kept in scene_utils.rs). `RenderAutoExposure`
//! and the `AddAutoExposure` layer measure the HDR scene colour's log-average luminance;
//! `TonemapFrameUpdate` turns the last measurement into the scalar shaders read as
//! `g_flToneMapScalarLinear`: `clamp(KEY / luminance, min, max) * compensation`. The colour is
//! measured before exposure (managed's tonemap pass applies it), so one frame settles it.
//! TODO: adapt over `Rate` instead of snapping to the target.

use crate::render::{readback, texture};
use abi::NativeRef;
use abi::structs::{NativeRect, SceneTonemapParameters_t};

/// Middle grey: the luminance the average is exposed to.
const KEY: f32 = 0.18;
/// Every `STEP`th texel in each direction is measured.
const STEP: usize = 4;
const RGBA16161616F: i32 = 24;
const RGBA8888: i32 = 0;

pub type Ref = NativeRef<abi::class::ITonemapSystem>;

pub struct State {
    pub params: Option<SceneTonemapParameters_t>,
    pub luminance: Option<f32>,
    pub scalar: f32,
}

impl Default for State {
    fn default() -> Self {
        State { params: None, luminance: None, scalar: 1.0 }
    }
}

#[derive(Default)]
pub struct Tonemap(pub std::sync::Mutex<State>);

fn with<R: Default>(t: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    objects::get::<_, Tonemap>(t).map(|s| f(&mut s.0.lock().unwrap())).unwrap_or_default()
}

/// The scalar for `luminance` under `p`; native's defaults (-1) leave that bound open.
pub fn target(p: &SceneTonemapParameters_t, luminance: f32) -> f32 {
    let (min, max) = (p.m_fl_auto_exposure_min, p.m_fl_auto_exposure_max);
    let min = if min > 0.0 { min } else { 0.0 };
    let max = if max >= min && max > 0.0 { max } else { f32::MAX };
    let compensation = if p.m_fl_exposure_compensation_scalar > 0.0 { p.m_fl_exposure_compensation_scalar } else { 1.0 };
    (KEY / luminance.max(1e-4)).clamp(min, max) * compensation
}

/// `TonemapFrameUpdate`: the scalar, moved to the last measurement's target when `update`.
pub fn frame_update(t: Ref, update: bool) -> f32 {
    with(t, |s| {
        if let (true, Some(p), Some(l)) = (update, s.params, s.luminance) {
            s.scalar = target(&p, l);
        }
        if crate::render::scene::trace::enabled() {
            let msg = format!("exposure: update {update} params {} luminance {:?} scalar {}", s.params.is_some(), s.luminance, s.scalar);
            abi::rt::log::log(abi::rt::log::Level::Info, &msg);
        }
        s.scalar
    })
}

fn half(bits: u16) -> f32 {
    let (sign, exp, frac) = ((bits >> 15) as u32, ((bits >> 10) & 0x1f) as i32, (bits & 0x3ff) as f32);
    let v = match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    };
    if sign == 1 { -v } else { v }
}

/// Log-average Rec. 709 luminance of `rect` in `color` (all of it for an empty rect).
pub fn measure(color: texture::Ref, rect: NativeRect) -> Option<f32> {
    let (frame, format) = readback::read(color, rect, 0)?;
    let rgb = |p: &[u8]| -> Option<[f32; 3]> {
        match format {
            RGBA16161616F => Some(std::array::from_fn(|i| half(u16::from_le_bytes([p[i * 2], p[i * 2 + 1]])))),
            RGBA8888 => Some(std::array::from_fn(|i| render::srgb::decode(f32::from(p[i]) / 255.0))),
            _ => None,
        }
    };
    let (mut sum, mut n) = (0.0f64, 0usize);
    for y in (0..frame.height).step_by(STEP) {
        for x in (0..frame.width).step_by(STEP) {
            let [r, g, b] = rgb(frame.texel(x, y))?;
            let l = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(1e-4, 65504.0);
            sum += f64::from(l.ln());
            n += 1;
        }
    }
    (n > 0).then(|| (sum / n as f64).exp() as f32)
}

/// Measure `color` for `t`'s next frame update.
pub fn record(t: Ref, color: texture::Ref, rect: NativeRect) {
    if let Some(l) = measure(color, rect) {
        with(t, |s| s.luminance = Some(l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_targets_middle_grey_within_bounds() {
        let p = SceneTonemapParameters_t {
            m_fl_auto_exposure_min: 1.0,
            m_fl_auto_exposure_max: 3.0,
            m_fl_exposure_compensation_scalar: 2.0,
            m_fl_tonemap_percent_target: -1.0,
            m_fl_tonemap_percent_bright_pixels: -1.0,
            m_fl_tonemap_min_avg_lum: -1.0,
            m_fl_rate: 1.0,
            m_fl_accelerate_exposure_down: -1.0,
            m_fl_exposure_adaptation_speed_up: -1.0,
            m_fl_exposure_adaptation_speed_down: -1.0,
            m_fl_tonemap_ev_smoothing_range: -1.0,
        };
        assert_eq!(target(&p, 0.09), 4.0, "0.18 / 0.09 = 2, times the compensation");
        assert_eq!(target(&p, 0.01), 6.0, "clamped to the maximum");
        assert_eq!(target(&p, 1.0), 2.0, "clamped to the minimum");
        assert_eq!((half(0x3c00), half(0xc000), half(0x0001)), (1.0, -2.0, 2f32.powi(-24)));
    }
}
