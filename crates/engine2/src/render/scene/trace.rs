//! `UNDERBOX_LAYERS=log`: one line per executed layer (view, layer name, kind, target formats and
//! sizes, draws), for finding where a booted client's frame goes black.

use super::layer::{Kind, SceneLayer};
use crate::render::draw::{Draw, Target};
use crate::render::texture;
use std::sync::LazyLock;

static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var("UNDERBOX_LAYERS").is_ok_and(|v| v == "log" || v == "probe"));

pub fn enabled() -> bool {
    *ENABLED
}

/// `UNDERBOX_SKIP_LAYERS=name,name`: layers not executed, for bisecting what covers a frame.
pub fn skipped(layer: &str) -> bool {
    static SKIP: LazyLock<Vec<String>> = LazyLock::new(|| std::env::var("UNDERBOX_SKIP_LAYERS").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default());
    SKIP.iter().any(|s| s == layer)
}

pub(crate) fn describe(t: texture::Ref) -> String {
    match texture::resolve(t) {
        Some(t) => match &t.gpu {
            Some(g) => format!("{}:{:?}:{}x{}", t.name, g.desc.format, g.desc.width, g.desc.height),
            None => format!("{}:cpu", t.name),
        },
        None if t.is_null() => "-".into(),
        None => "dead".into(),
    }
}

/// One line per `DrawModelDrawCall` (the first 8 per pair of targets): targets, viewport, depth
/// mode, instance ids and the transform each reads.
pub fn draw_call(t: &Target, reverse_z: bool, ids: &[u32], transforms: Option<&[u8]>) {
    static SEEN: LazyLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> = LazyLock::new(Default::default);
    let targets = format!("{} / {}", describe(t.color), describe(t.depth));
    let n = {
        let mut seen = SEEN.lock().unwrap();
        let n = seen.entry(targets.clone()).or_default();
        *n += 1;
        *n
    };
    if !enabled() || n > 8 {
        return;
    }
    let f = |b: &[u8], i: usize| b.get(i * 4..i * 4 + 4).map_or(f32::NAN, |x| f32::from_le_bytes(x.try_into().unwrap()));
    let at: Vec<String> = ids.iter().map(|&id| transforms.map_or("-".into(), |b| { let o = id as usize * 16; format!("{id}@({:.0},{:.0},{:.0})", f(b, o + 3), f(b, o + 7), f(b, o + 11)) })).collect();
    let v = t.viewport;
    let msg = format!(
        "draw call: -> {targets} viewport {}x{}+{}+{} z {}..{} reverse_z {reverse_z} instances [{}] transforms {}",
        v.m_n_width, v.m_n_height, v.m_n_top_left_x, v.m_n_top_left_y, v.min_z, v.max_z, at.join(" "), transforms.map_or(0, <[u8]>::len)
    );
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}

/// The view's eye and forward axis, before its layers.
pub fn view(view: usize, view_proj: glam::Mat4) {
    if !enabled() {
        return;
    }
    let v = crate::render::shading::view::View { view_proj, viewport: [0.0; 4], target: [1.0; 2], tone_map: 1.0 };
    let (eye, forward) = (v.camera_position(), v.camera_axes().0);
    abi::rt::log::log(abi::rt::log::Level::Info, &format!("view: {view:#x} eye ({:.0},{:.0},{:.0}) forward ({:.2},{:.2},{:.2})", eye.x, eye.y, eye.z, forward.x, forward.y, forward.z));
}

/// Each distinct (model, position, scale) an opaque layer draws, once per run.
pub fn object(s: &crate::scene::object::State) {
    static SEEN: LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> = LazyLock::new(Default::default);
    if !enabled() {
        return;
    }
    let (p, k) = (s.transform.position.vec, s.transform.scale.vec);
    let model = crate::model::get(s.model).map_or_else(|| "-".into(), |m| m.name.clone());
    let msg = format!("object: {model} at ({:.0},{:.0},{:.0}) scale ({:.2},{:.2},{:.2}) flags {:#x}", p.x, p.y, p.z, k.x, k.y, k.z, s.flags);
    if SEEN.lock().unwrap().insert(msg.clone()) {
        abi::rt::log::log(abi::rt::log::Level::Info, &msg);
    }
}

/// `UNDERBOX_LAYERS=probe`: after each layer, the colour target's centre and lower-centre texels.
pub fn probe(view: usize, layer: &SceneLayer, t: &Target) {
    static ON: LazyLock<bool> = LazyLock::new(|| std::env::var("UNDERBOX_LAYERS").is_ok_and(|v| v == "probe"));
    let (true, Some(gpu), Some(tex)) = (*ON, crate::render::device::gpu(), texture::resolve(t.color)) else { return };
    let Some(g) = tex.gpu.as_ref() else { return };
    let (w, h) = (g.desc.width, g.desc.height);
    let texel = |x: u32, y: u32| -> String {
        let Some(f) = g.read_region(gpu, 0, [x, y, 1, 1]) else { return "?".into() };
        let p = &f.pixels;
        match g.desc.format {
            render::wgpu::TextureFormat::Rgba16Float => {
                let c = |i: usize| half::f16::from_bits(u16::from_le_bytes([p[i * 2], p[i * 2 + 1]])).to_f32();
                format!("({:.3},{:.3},{:.3},{:.3})", c(0), c(1), c(2), c(3))
            }
            _ => format!("{:?}", &p[..p.len().min(4)]),
        }
    };
    let msg = format!("layer probe: view {view:#x} '{}' {} centre {} low {}", layer.name, describe(t.color), texel(w / 2, h / 2), texel(w / 2, h * 7 / 8));
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}

pub fn layer(view: usize, layer: &SceneLayer, t: &Target, draws: &[Draw]) {
    if !enabled() {
        return;
    }
    let kind = match layer.kind {
        Kind::Render(mode) => format!("render({:#x})", mode.0),
        Kind::Clear(f) => format!("clear({f:#x})"),
        Kind::Label => "label".into(),
        Kind::Procedural(_) => "procedural".into(),
        Kind::Blit { src, .. } => format!("blit({})", describe(src)),
        Kind::View(child) => format!("view({:#x})", child.addr()),
        Kind::Exposure { src, .. } => format!("exposure({})", describe(src)),
    };
    let msg = format!("layer: view {view:#x} '{}' {kind} -> {} / {} draws {} with material {}", layer.name, describe(t.color), describe(t.depth), draws.len(), draws.iter().filter(|d| d.shaded.is_some()).count());
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}
