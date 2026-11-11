//! `ImageLoader.ConvertImageFormat`: uncompressed pixels from one `ImageFormat` to another, as
//! ScreenshotService does for swap chains that aren't RGBA8888 (an HDR chain reads back RGBA16F).
//! Values are converted, never colour-managed (no sRGB transfer), like native's converter; wider
//! formats clamp and round into narrower ones. Block-compressed, depth and packed (565, 1010102,
//! ...) formats aren't converted: the call fails.

use abi::RawPtr;
use std::ffi::c_void;

#[derive(Clone, Copy)]
enum Channels {
    /// Byte offsets of R, G, B, A in a pixel; `None` reads as 0 (colour) or 1 (alpha).
    U8([Option<usize>; 4]),
    /// Grey (`I8`), alpha (`A8`) or grey + alpha (`IA88`).
    Grey { alpha: bool, grey: bool },
    /// The first n of RGBA as 16-bit unorm, half or 32-bit float.
    U16(usize),
    F16(usize),
    F32(usize),
}

/// Bytes per pixel and channel layout of the formats we convert.
fn layout(format: i64) -> Option<(usize, Channels)> {
    use Channels::*;
    let rgba = |r, g, b, a| U8([Some(r), Some(g), Some(b), a]);
    Some(match format {
        0 | 42 | 81 => (4, rgba(0, 1, 2, Some(3))),
        1 | 43 => (4, rgba(3, 2, 1, Some(0))),
        2 | 46 => (3, rgba(0, 1, 2, None)),
        3 | 47 => (3, rgba(2, 1, 0, None)),
        11 | 44 => (4, rgba(1, 2, 3, Some(0))),
        12 | 45 | 52 | 82 => (4, rgba(2, 1, 0, Some(3))),
        16 | 41 | 51 | 83 => (4, rgba(2, 1, 0, None)),
        55 => (4, rgba(0, 1, 2, None)),
        5 | 49 => (1, Grey { alpha: false, grey: true }),
        8 => (1, Grey { alpha: true, grey: false }),
        6 => (2, Grey { alpha: true, grey: true }),
        25 | 50 => (8, U16(4)),
        79 => (4, U16(2)),
        80 => (2, U16(1)),
        24 => (8, F16(4)),
        53 => (4, F16(2)),
        59 => (2, F16(1)),
        29 => (16, F32(4)),
        28 => (12, F32(3)),
        54 => (8, F32(2)),
        27 => (4, F32(1)),
        _ => return None,
    })
}

fn decode(c: Channels, p: &[u8]) -> [f32; 4] {
    let u16_at = |i: usize| u16::from_le_bytes([p[i * 2], p[i * 2 + 1]]);
    let first = |n: usize, at: &dyn Fn(usize) -> f32| std::array::from_fn(|i| if i < n { at(i) } else if i == 3 { 1.0 } else { 0.0 });
    match c {
        Channels::U8(at) => {
            let ch = |i: usize, missing: f32| at[i].map_or(missing, |o| f32::from(p[o]) / 255.0);
            [ch(0, 0.0), ch(1, 0.0), ch(2, 0.0), ch(3, 1.0)]
        }
        Channels::Grey { alpha, grey } => {
            let g = if grey { f32::from(p[0]) / 255.0 } else { 0.0 };
            let a = if alpha { f32::from(p[usize::from(grey)]) / 255.0 } else { 1.0 };
            [g, g, g, a]
        }
        Channels::U16(n) => first(n, &|i| f32::from(u16_at(i)) / 65535.0),
        Channels::F16(n) => first(n, &|i| half::f16::from_bits(u16_at(i)).to_f32()),
        Channels::F32(n) => first(n, &|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap())),
    }
}

fn encode(c: Channels, v: [f32; 4], out: &mut [u8]) {
    let unorm8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    match c {
        Channels::U8(at) => at.iter().zip(v).for_each(|(o, x)| {
            if let Some(o) = o {
                out[*o] = unorm8(x);
            }
        }),
        Channels::Grey { alpha, grey } => {
            if grey {
                out[0] = unorm8(0.299 * v[0] + 0.587 * v[1] + 0.114 * v[2]);
            }
            if alpha {
                out[usize::from(grey)] = unorm8(v[3]);
            }
        }
        Channels::U16(n) => (0..n).for_each(|i| out[i * 2..i * 2 + 2].copy_from_slice(&((v[i].clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes())),
        Channels::F16(n) => (0..n).for_each(|i| out[i * 2..i * 2 + 2].copy_from_slice(&half::f16::from_f32(v[i]).to_bits().to_le_bytes())),
        Channels::F32(n) => (0..n).for_each(|i| out[i * 4..i * 4 + 4].copy_from_slice(&v[i].to_le_bytes())),
    }
}

/// An image's buffer, its format and its row stride in bytes (0: rows are packed).
pub struct Pixels<B> {
    pub bytes: B,
    pub format: i64,
    pub stride: usize,
}

/// Converts `w`x`h` pixels. False for a format we don't convert or a buffer too small.
pub fn convert_pixels(src: Pixels<&[u8]>, dst: Pixels<&mut [u8]>, w: usize, h: usize) -> bool {
    let (Some((sb, sc)), Some((db, dc))) = (layout(src.format), layout(dst.format)) else { return false };
    let stride = |s: usize, bpp: usize| if s == 0 { w * bpp } else { s };
    let (ss, ds) = (stride(src.stride, sb), stride(dst.stride, db));
    if h == 0 || src.bytes.len() < ss * (h - 1) + w * sb || dst.bytes.len() < ds * (h - 1) + w * db {
        return h == 0;
    }
    for y in 0..h {
        let (srow, drow) = (&src.bytes[y * ss..y * ss + w * sb], &mut dst.bytes[y * ds..y * ds + w * db]);
        if src.format == dst.format {
            drow.copy_from_slice(srow);
            continue;
        }
        for (s, d) in srow.chunks_exact(sb).zip(drow.chunks_exact_mut(db)) {
            encode(dc, decode(sc, s), d);
        }
    }
    true
}

/// The slot: a stride of 0 or less means tightly packed.
#[allow(clippy::too_many_arguments)]
pub fn convert(src: RawPtr<c_void>, from: i64, dst: RawPtr<c_void>, to: i64, w: i32, h: i32, src_stride: i32, dst_stride: i32) -> bool {
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    let (Some((sb, _)), Some((db, _))) = (layout(from), layout(to)) else { return false };
    let (ss, ds) = (if src_stride <= 0 { w * sb } else { src_stride as usize }, if dst_stride <= 0 { w * db } else { dst_stride as usize });
    if src.is_null() || dst.is_null() || w == 0 || h == 0 {
        return false;
    }
    // SAFETY: managed passes buffers of `stride * height` bytes for each image (ScreenshotService).
    let (s, d) = unsafe { (src.cast::<u8>().as_slice(ss * h), dst.cast::<u8>().as_mut_slice(ds * h)) };
    convert_pixels(Pixels { bytes: s, format: from, stride: ss }, Pixels { bytes: d, format: to, stride: ds }, w, h)
}

#[cfg(test)]
#[path = "image_convert_tests.rs"]
mod tests;
