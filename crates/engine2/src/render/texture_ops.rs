//! `RenderTools::CopyTexture`, `IRenderContext::GenerateMipMaps` and `ClearTextureUint` on
//! textures' GPU sides.

use crate::render::{device, texture};
use render::Subresource;
use std::sync::OnceLock;

/// (source mip, source slice, destination mip, destination slice), as managed passes them.
pub type Slices = [u32; 4];

/// `rect` (`None`: all of the source mip) of one source mip and slice into one destination mip and
/// slice at `to`. With every index 0, no rect and no offset, the whole resource: every mip and
/// layer both have (managed's `Graphics.CopyTexture(src, dst)`).
pub fn copy(src: texture::Ref, dst: texture::Ref, rect: Option<[u32; 4]>, to: (u32, u32), [sm, ss, dm, ds]: Slices) -> bool {
    let (Some(gpu), Some(s), Some(d)) = (device::gpu(), texture::resolve(src), texture::resolve(dst)) else { return false };
    let (Some(a), Some(b)) = (s.gpu.as_ref(), d.gpu.as_ref()) else { return false };
    let at = |mip, layer| Subresource { mip, layer };
    let whole = |m: u32| {
        let (w, h) = a.mip_size(m);
        [0, 0, w, h]
    };
    if rect.is_none() && [sm, ss, dm, ds] == [0; 4] && to == (0, 0) {
        let mips = a.desc.mips.min(b.desc.mips).max(1);
        return (0..mips).all(|m| a.copy_region(gpu, b, (at(m, 0), at(m, 0)), a.mip_layers(m).min(b.mip_layers(m)), whole(m), (0, 0)));
    }
    a.copy_region(gpu, b, (at(sm, ss), at(dm, ds)), 1, rect.unwrap_or_else(|| whole(sm)), to)
}

/// Redraw mips 1 and up from mip 0 (render's `MipGenerator`); false when the texture can't take it.
pub fn generate_mips(t: texture::Ref) -> bool {
    static GENERATOR: OnceLock<render::MipGenerator> = OnceLock::new();
    let (Some(gpu), Some(t)) = (device::gpu(), texture::resolve(t)) else { return false };
    let Some(tex) = t.gpu.as_ref() else { return false };
    GENERATOR.get_or_init(|| render::MipGenerator::new(gpu)).generate(gpu, tex)
}

/// Every component of every texel set to `value`, cut to the component's size (an `R32Uint`
/// texel is `value`, an `Rgba8Uint` one four times its low byte), on the GPU and in the CPU copy.
pub fn clear_uint(t: texture::Ref, value: u32) -> bool {
    let Some(t) = texture::resolve(t) else { return false };
    let Some(tex) = t.gpu.as_ref() else { return false };
    let Some(size) = tex.texel_size() else { return false };
    let components = u32::from(tex.desc.format.components()).max(1);
    let per = (size / components) as usize;
    let bytes = u64::from(value).to_le_bytes();
    let texel: Vec<u8> = (0..components).flat_map(|_| bytes[..per.min(8)].iter().copied()).collect();
    t.fill_cpu(&texel);
    device::gpu().is_some_and(|gpu| tex.fill(gpu, &texel))
}

#[cfg(test)]
#[path = "texture_ops_tests.rs"]
mod tests;
