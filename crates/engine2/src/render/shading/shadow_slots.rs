//! Managed's local-light shadow maps for the stand-in. Its `ProjectedShadows` and
//! `ProjectedCubeShadows` name maps by bindless index, but naga can't compare-sample a binding
//! array, so the stand-in declares [`SLOTS`] plain depth textures of each shape instead: slot `k`
//! holds the `k`th live depth texture of that shape in bindless order, and
//! `g_vUnderboxShadow{2D,Cube}Indices.k` its index (0, which is never handed out, when empty).
//! Maps past the slots go unshadowed.

use crate::render::{bindless, texture};
use crate::resource::Resource as _;
use render::wgpu::TextureViewDimension as D;

pub const SLOTS: usize = 4;
const SHAPES: [(&str, &str, D); 2] = [("g_tUnderboxShadow2D", "g_vUnderboxShadow2DIndices", D::D2), ("g_tUnderboxShadowCube", "g_vUnderboxShadowCubeIndices", D::Cube)];

fn slots(dimension: D) -> Vec<(u32, texture::Ref)> {
    let fits = |t: texture::Ref| texture::resolve(t).filter(|t| !t.is_error()).and_then(|t| t.gpu.as_ref().map(|g| g.dimension() == dimension && g.desc.format.is_depth_stencil_format())).unwrap_or(false);
    bindless::entries().into_iter().filter(|(_, t)| fits(*t)).take(SLOTS).collect()
}

/// The texture a slot binding (`g_tUnderboxShadow2D0`..) names.
pub fn texture(name: &str) -> Option<texture::Ref> {
    let (prefix, _, dimension) = SHAPES.iter().find(|(p, _, _)| name.starts_with(p))?;
    let k: usize = name[prefix.len()..].parse().ok()?;
    slots(*dimension).get(k).map(|(_, t)| *t)
}

/// The slots' bindless indices (`g_vUnderboxShadow2DIndices`, `g_vUnderboxShadowCubeIndices`).
pub fn indices(name: &str) -> Option<glam::Vec4> {
    let (_, _, dimension) = SHAPES.iter().find(|(_, n, _)| *n == name)?;
    let mut v = [0.0; SLOTS];
    for (k, (i, _)) in slots(*dimension).into_iter().enumerate() {
        v[k] = i as f32;
    }
    Some(glam::Vec4::from_array(v))
}
