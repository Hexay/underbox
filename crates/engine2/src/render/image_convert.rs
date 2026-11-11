//! `ImageLoader.ConvertImageFormat` between the 8-bit-per-channel formats (screenshots, texture
//! readback): every pixel through RGBA8. Compressed and float formats answer false.

use abi::RawPtr;
use std::ffi::c_void;

/// Byte positions of R, G, B and A in a pixel of `format` (`None`: no alpha, opaque), and its size.
fn channels(format: i64) -> Option<([Option<usize>; 4], usize)> {
    Some(match format {
        0 => ([Some(0), Some(1), Some(2), Some(3)], 4), // RGBA8888
        1 => ([Some(3), Some(2), Some(1), Some(0)], 4), // ABGR8888
        2 => ([Some(0), Some(1), Some(2), None], 3),    // RGB888
        3 => ([Some(2), Some(1), Some(0), None], 3),    // BGR888
        5 => ([Some(0), Some(0), Some(0), None], 1),    // I8
        6 => ([Some(0), Some(0), Some(0), Some(1)], 2), // IA88
        8 => ([None, None, None, Some(0)], 1),          // A8
        11 => ([Some(1), Some(2), Some(3), Some(0)], 4), // ARGB8888
        12 => ([Some(2), Some(1), Some(0), Some(3)], 4), // BGRA8888
        16 => ([Some(2), Some(1), Some(0), None], 4),   // BGRX8888
        _ => return None,
    })
}

fn decode(px: &[u8], layout: &[Option<usize>; 4]) -> [u8; 4] {
    std::array::from_fn(|c| layout[c].map_or(if c == 3 { 255 } else { 0 }, |i| px[i]))
}

/// Writes RGBA into `px` laid out as `layout` (`I8`/`IA88` take the luminance; padding is 255).
fn encode(px: &mut [u8], layout: &[Option<usize>; 4], rgba: [u8; 4]) {
    px.fill(255);
    let grey = layout[0] == layout[1];
    for c in (0..4).rev() {
        if let Some(i) = layout[c] {
            px[i] = if grey && c < 3 { ((u32::from(rgba[0]) * 77 + u32::from(rgba[1]) * 150 + u32::from(rgba[2]) * 29) >> 8) as u8 } else { rgba[c] };
        }
    }
}

/// Convert a `w`x`h` image; a stride of 0 means tightly packed.
#[allow(clippy::too_many_arguments)]
pub fn convert(src: RawPtr<c_void>, from: i64, dst: RawPtr<c_void>, to: i64, w: i32, h: i32, src_stride: i32, dst_stride: i32) -> bool {
    let (Some((sl, sb)), Some((dl, db))) = (channels(from), channels(to)) else { return false };
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    let stride = |given: i32, bytes: usize| if given > 0 { given as usize } else { w * bytes };
    let (ss, ds) = (stride(src_stride, sb), stride(dst_stride, db));
    if src.is_null() || dst.is_null() || ss < w * sb || ds < w * db {
        return false;
    }
    // SAFETY: managed passes pinned buffers sized for `h` rows of these strides (Texture.Read.cs).
    let (src, dst) = unsafe { (src.cast::<u8>().as_slice(ss * h), dst.cast::<u8>().as_mut_slice(ds * h)) };
    for y in 0..h {
        for x in 0..w {
            let rgba = decode(&src[y * ss + x * sb..][..sb], &sl);
            encode(&mut dst[y * ds + x * db..][..db], &dl, rgba);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_and_grey() {
        let src = [10u8, 20, 30, 40, 1, 2, 3, 4];
        let mut rgba = [0u8; 8];
        assert!(convert(RawPtr::from_raw(src.as_ptr().cast_mut().cast()), 12, RawPtr::from_raw(rgba.as_mut_ptr().cast()), 0, 2, 1, 0, 8));
        assert_eq!(rgba, [30, 20, 10, 40, 3, 2, 1, 4]);
        let mut grey = [0u8; 2];
        assert!(convert(RawPtr::from_raw(rgba.as_ptr().cast_mut().cast()), 0, RawPtr::from_raw(grey.as_mut_ptr().cast()), 5, 2, 1, 0, 0));
        assert_eq!(grey[0], ((30 * 77 + 20 * 150 + 10 * 29) >> 8) as u8);
        assert!(!convert(RawPtr::from_raw(src.as_ptr().cast_mut().cast()), 13, RawPtr::from_raw(rgba.as_mut_ptr().cast()), 0, 2, 1, 0, 0), "DXT1 isn't converted");
    }
}
