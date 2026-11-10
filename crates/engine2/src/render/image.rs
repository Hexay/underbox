//! Image formats (`NativeEngine.ImageFormat`, Sandbox.Engine/Core/Interop/NativeEngine/ImageFormat.cs)
//! and the memory sizes `ImageLoader` reports for them.

use abi::slots::image_loader as loader;
use abi::{Registry, bind};

/// How one format lays out texels: per pixel, or in 4x4 compressed blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Pixel { bytes: u32 },
    Block4x4 { bytes: u32 },
}

pub fn layout(format: i64) -> Option<Layout> {
    use Layout::*;
    Some(match format {
        // RGBA8888 ABGR8888 ARGB8888 BGRA8888 BGRX8888, the LINEAR_/LE_ 32-bit ones, RGBX8888,
        // 10:10:10:2, D32, D24* and the *_LINEAR 32-bit ones, R32_UINT, R11G11B10F.
        0 | 1 | 11 | 12 | 16 | 41..=45 | 51 | 52 | 55 | 57 | 58 | 62..=67 | 81..=83 | 87 | 96 => {
            Pixel { bytes: 4 }
        }
        2 | 3 | 46 | 47 => Pixel { bytes: 3 },
        // RG1616F, RG1616, R32F.
        27 | 53 | 79 => Pixel { bytes: 4 },
        // 565, 5551, 4444, IA88, R16F, D16, D15S1, R16, RGBX555, I16F.
        4 | 6 | 17..=19 | 21 | 48 | 59 | 60 | 61 | 80 | 84 | 94 => Pixel { bytes: 2 },
        5 | 8 | 49 => Pixel { bytes: 1 },
        // RGBA16161616(F), RG3232(F), D32FS8.
        24 | 25 | 50 | 54 | 92 | 95 => Pixel { bytes: 8 },
        28 => Pixel { bytes: 12 },
        29 | 93 => Pixel { bytes: 16 },
        // DXT1, DXT1_ONEBITALPHA, ATI1N, ETC2 RGB, R11_EAC.
        13 | 20 | 37 | 88 | 90 => Block4x4 { bytes: 8 },
        // DXT3, DXT5, ATI2N, DXT5_NM, BC6H, BC7, ETC2 RGBA, RG11_EAC.
        14 | 15 | 36 | 78 | 85 | 86 | 89 | 91 => Block4x4 { bytes: 16 },
        _ => return None,
    })
}

/// Bytes one mip level of `w`x`h`x`d` takes in `format`; 0 for an unknown format.
pub fn level_size(w: u32, h: u32, d: u32, format: i64) -> u64 {
    let (w, h, d) = (w.max(1) as u64, h.max(1) as u64, d.max(1) as u64);
    match layout(format) {
        Some(Layout::Pixel { bytes }) => w * h * d * bytes as u64,
        Some(Layout::Block4x4 { bytes }) => w.div_ceil(4) * h.div_ceil(4) * d * bytes as u64,
        None => 0,
    }
}

/// Bytes for `mips` levels starting at full size (`None`: the whole chain down to 1x1x1).
pub fn mem_required(w: i32, h: i32, d: i32, mips: Option<u32>, format: i64) -> i32 {
    let (mut w, mut h, mut d) = (w.max(0) as u32, h.max(0) as u32, d.max(0) as u32);
    if w == 0 || h == 0 {
        return 0;
    }
    let mut total = 0u64;
    let mut level = 0;
    loop {
        total += level_size(w, h, d, format);
        level += 1;
        let last = (w, h, d) == (1, 1, 1);
        if mips.map_or(last, |m| level >= m.max(1)) {
            break;
        }
        (w, h, d) = ((w / 2).max(1), (h / 2).max(1), (d / 2).max(1));
    }
    i32::try_from(total).unwrap_or(i32::MAX)
}

pub fn register(r: &mut Registry) {
    bind!(r, loader::GET_MEM_REQUIRED, |w, h, d, format, mipmap: bool| {
        mem_required(w, h, d, if mipmap { None } else { Some(1) }, format)
    });
    bind!(r, loader::GET_MEM_REQUIRED_1, |w, h, d, mips, format| {
        mem_required(w, h, d, Some(mips.max(1) as u32), format)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(mem_required(1, 1, 1, Some(1), 0), 4);
        assert_eq!(mem_required(4, 4, 1, None, 0), 64 + 16 + 4);
        assert_eq!(mem_required(5, 5, 1, Some(1), 13), 4 * 8);
        assert_eq!(mem_required(16, 16, 1, Some(1), -2), 0);
    }
}
