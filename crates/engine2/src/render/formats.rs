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
        // DXT1, DXT5, ATI2N, ATI1N, BC6H, BC7: block-compressed, uploaded as stored (compiled/texture.rs).
        13 => F::Bc1RgbaUnorm,
        15 => F::Bc3RgbaUnorm,
        36 => F::Bc5RgUnorm,
        37 => F::Bc4RUnorm,
        85 => F::Bc6hRgbUfloat,
        86 => F::Bc7RgbaUnorm,
        // A8: the contact-shadow mask (ContactShadowLayer), R8 on real.
        8 => F::R8Unorm,
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
        92 if crate::render::device::gpu().is_none_or(|g| g.device.features().contains(render::wgpu::Features::DEPTH32FLOAT_STENCIL8)) => F::Depth32FloatStencil8,
        // D24S8, LINEAR_D24S8, D24FS8, and D32FS8 without the optional feature.
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
            let (Some(f), Some(layout)) = (to_wgpu(format), crate::render::image::layout(format.into())) else { continue };
            let bytes = match layout {
                crate::render::image::Layout::Pixel { bytes } | crate::render::image::Layout::Block4x4 { bytes } => bytes,
            };
            if let Some(size) = f.block_copy_size(None) {
                assert_eq!(size, bytes, "format {format} -> {f:?}");
            }
        }
    }
}
