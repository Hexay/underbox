//! A sprite-sheet texture: a 2×2 grid of 8×8 cells (red, green, blue, white) with a looping
//! three-frame sequence over the first three cells and a clamped one-frame sequence on the last.

use super::super::texture::sheet::{Frame, Sequence};
use super::super::texture::{Format, Texture};

pub const SHEET_TEXTURE: &str = "materials/compiled/sheet.vtex";
pub const SHEET_CELL: usize = 8;
pub const SHEET_COLORS: [[u8; 4]; 4] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]];

/// Cell `i`'s UVs, cropped and uncropped alike.
pub fn sheet_cell_uv(i: usize) -> [f32; 8] {
    let (u, v) = ((i % 2) as f32 * 0.5, (i / 2) as f32 * 0.5);
    [u, v, u + 0.5, v + 0.5, u, v, u + 0.5, v + 0.5]
}

pub fn sheet_sequences() -> Vec<Sequence> {
    let frame = |i: usize, t: f32| Frame { display_time: t, images: vec![sheet_cell_uv(i)] };
    vec![
        Sequence { id: 0, total_time: 0.3, name: "cycle".into(), frames: (0..3).map(|i| frame(i, 0.1)).collect(), ..Sequence::default() },
        Sequence { id: 1, clamp: true, total_time: 1.0, name: "still".into(), frames: vec![frame(3, 1.0)], ..Sequence::default() },
    ]
}

pub fn sprite_sheet() -> Texture {
    let size = SHEET_CELL * 2;
    let texels: Vec<u8> = (0..size * size).flat_map(|i| SHEET_COLORS[(i % size / SHEET_CELL) + 2 * (i / size / SHEET_CELL)]).collect();
    Texture { width: size as u16, height: size as u16, depth: 1, format: Format::RGBA8888, levels: vec![texels], sheet: sheet_sequences(), ..Texture::default() }
}
