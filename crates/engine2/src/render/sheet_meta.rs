//! GPU sprite sheets: every sheeted texture's sequences in one `RGBA32F` texture that
//! `GetSheetMetaDataTexture` hands out and `sheet_sampling.fxc` (crates/shaders/hlsl/builtin) reads.
//! Real's layout is closed; this one is ours, addressed through the same `GetSheetInfo` vector
//! `(base V, width, 1 / height, sequence count)` managed passes to `Sheet.hlsl`.
//!
//! Each sheet takes one row per sequence, contiguous, rows never moving while the sheet lives (so
//! a base V handed out stays valid). A row holds texel 0 = (frame count, total time, clamp, 0),
//! then one texel per frame with its first image's cropped UV bounds (min xy, max zw), then one per
//! frame with (start time, display time, 0, 0). Frames past [`MAX_FRAMES`] are dropped.

use crate::render::texture;
use abi::NativeRef;
use abi::structs::{Rect3D, TextureCreationConfig_t, Vector4};
use assets::compiled::texture::sheet::Sequence;
use std::sync::{Mutex, OnceLock};

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 256;
pub const MAX_FRAMES: usize = (WIDTH - 1) / 2;
/// `ImageFormat.RGBA32323232F`.
const RGBA32F: i32 = 29;

struct Meta {
    texture: usize,
    used: [bool; HEIGHT],
}

fn meta() -> &'static Mutex<Meta> {
    static META: OnceLock<Mutex<Meta>> = OnceLock::new();
    META.get_or_init(|| {
        let mut config = TextureCreationConfig_t::default();
        (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (WIDTH as i16, HEIGHT as i16, 1, 1);
        config.m_n_image_format = RGBA32F;
        let mut zeros = vec![0u8; WIDTH * HEIGHT * 16];
        let t = texture::create(Some(c"sheet metadata"), true, config, &mut zeros);
        Mutex::new(Meta { texture: t.addr(), used: [false; HEIGHT] })
    })
}

/// `GetSheetMetaDataTexture`: made once, never freed.
pub fn texture() -> texture::Ref {
    NativeRef::from_raw(meta().lock().unwrap().texture as *mut _)
}

/// A sheet's rows in the metadata texture, given back when dropped.
#[derive(Debug)]
pub struct Rows {
    first: usize,
    count: usize,
}

impl Rows {
    /// `GetSheetInfo`'s vector for these rows.
    pub fn info(&self) -> Vector4 {
        crate::math::vec4((self.first as f32 + 0.5) / HEIGHT as f32, WIDTH as f32, 1.0 / HEIGHT as f32, self.count as f32)
    }
}

impl Drop for Rows {
    fn drop(&mut self) {
        meta().lock().unwrap().used[self.first..self.first + self.count].fill(false);
    }
}

/// One sequence's row: see the module docs.
pub fn row(s: &Sequence) -> Vec<[f32; 4]> {
    let frames = &s.frames[..s.frames.len().min(MAX_FRAMES)];
    let mut out = vec![[0.0; 4]; WIDTH];
    let total: f32 = frames.iter().map(|f| f.display_time.max(0.0)).sum();
    out[0] = [frames.len() as f32, total, f32::from(u8::from(s.clamp)), 0.0];
    let mut start = 0.0;
    for (i, f) in frames.iter().enumerate() {
        let b = f.images.first().map_or([0.0, 0.0, 1.0, 1.0], |m| [m[0], m[1], m[2], m[3]]);
        out[1 + i] = b;
        out[1 + frames.len() + i] = [start, f.display_time.max(0.0), 0.0, 0.0];
        start += f.display_time.max(0.0);
    }
    out
}

/// Rows for `sequences`, written into the texture; `None` without sequences or room.
pub fn allocate(sequences: &[Sequence]) -> Option<Rows> {
    let count = sequences.len();
    let t = {
        let mut m = meta().lock().unwrap();
        let first = (0..=HEIGHT.checked_sub(count)?).find(|&r| count > 0 && m.used[r..r + count].iter().all(|u| !u))?;
        m.used[first..first + count].fill(true);
        (first, m.texture)
    };
    let (first, tex) = t;
    let bytes: Vec<u8> = sequences.iter().flat_map(row).flatten().flat_map(f32::to_le_bytes).collect();
    let rect = Rect3D { x: 0, y: first as i32, z: 0, width: WIDTH as i32, height: count as i32, depth: 1 };
    texture::set_data(NativeRef::from_raw(tex as *mut _), &bytes, rect);
    Some(Rows { first, count })
}

#[cfg(test)]
#[path = "sheet_meta_tests.rs"]
mod tests;
