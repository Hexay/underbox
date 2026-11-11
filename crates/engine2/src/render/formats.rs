//! `NativeEngine.ImageFormat` to wgpu formats, for the formats render targets and uploads use.

use render::wgpu::TextureFormat as F;

/// TODO: s&box's gamma formats (RGBA8888, BGRA8888) should sample as sRGB once materials draw;
/// they map to UNORM for now so cleared and read-back bytes match exactly.
pub fn to_wgpu(format: i32) -> Option<F> {
    Some(match format {
        // RGBA8888, LINEAR_RGBA8888, RGBA8888_LINEAR, RGBX8888.
        0 | 42 | 55 | 81 => F::Rgba8Unorm,
        // BGRA8888, LINEAR_BGRA8888, LE_BGRA8888, BGRA8888_LINEAR.
        12 | 45 | 52 | 82 => F::Bgra8Unorm,
        24 => F::Rgba16Float,
        27 => F::R32Float,
        29 => F::Rgba32Float,
        53 => F::Rg16Float,
        // RG3232F: the depth chain (SharedPasses.DepthChainTarget).
        54 => F::Rg32Float,
        57 => F::Rgb10a2Unorm,
        59 => F::R16Float,
        60 => F::Depth16Unorm,
        62 => F::Depth32Float,
        // D24S8, LINEAR_D24S8, D24FS8, D32FS8: Depth32FloatStencil8 needs an optional feature.
        63 | 64 | 67 | 92 => F::Depth24PlusStencil8,
        87 => F::R32Uint,
        96 => F::Rg11b10Ufloat,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_sizes_match_image_layouts() {
        for format in 0..100 {
            let (Some(f), Some(crate::render::image::Layout::Pixel { bytes })) = (to_wgpu(format), crate::render::image::layout(format.into())) else { continue };
            if let Some(size) = f.block_copy_size(None) {
                assert_eq!(size, bytes, "format {format} -> {f:?}");
            }
        }
    }
}
