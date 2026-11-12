//! A view's shadows through managed's shadow mapper, driven as native's light binner drives it
//! (ShadowMapperCallbacks.cs): `InitForView`, the sun (`DoDirectionalLight`), every shadowed
//! point and spot light (`FindOrCreateShadowMaps`, whose index becomes the light's
//! `ShadowMapIndex`), then `UploadToGPU` and `SetShaderAttributes`. The mapper fits the cascades,
//! sizes and time-slices the local maps, and draws every shadow view through `AddShadowView`
//! (render/scene/shadow_view.rs). Its `DirectionalLightCB`, `ProjectedShadows` and
//! `ProjectedCubeShadows` land on one attributes object every draw of the view binds from
//! (`Lighting::shadow_attributes`). Its maps are reverse Z (render/depth.rs).

use super::Lighting;
use crate::render::scene::view;
use crate::render::{attributes, texture};
use abi::callbacks::sandbox_rendering_shadow_mapper_callbacks as mapper;
use abi::{HandleId, RawPtr};
use glam::Mat4;
use std::sync::OnceLock;

/// Managed's "no shadow" index (`ShadowMapper.InvalidShadowIndex`).
const INVALID: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunShadow {
    /// World → (u, v, depth) in the shadow map.
    pub world_to_shadow: Mat4,
    pub map: texture::Ref,
    /// Nearer is larger (managed's cascades).
    pub reverse_z: bool,
}

/// The mapper's key: native passes its pooled light binner. Views render one at a time here, so
/// one binner serves them all.
fn binner() -> RawPtr<std::ffi::c_void> {
    static BINNER: u8 = 0;
    RawPtr::from_raw((&raw const BINNER).cast_mut().cast())
}

/// Where `SetShaderAttributes` writes, reused by every view.
fn shadow_attributes() -> attributes::Ref {
    static ATTRS: OnceLock<usize> = OnceLock::new();
    attributes::Ref::from_raw(*ATTRS.get_or_init(|| attributes::create().addr()) as *mut _)
}

/// Have managed's shadow mapper render `lighting`'s shadows for view `v` (which draws them through
/// `AddShadowView`), and record what it hands back on `lighting`. Nothing without managed.
pub fn render(lighting: &mut Lighting, v: view::Ref) {
    if !(38..=42).all(abi::rt::callbacks::is_set) {
        return;
    }
    let Some(camera) = view::get(v).map(|view| view.camera) else { return };
    let b = binner();
    mapper::init_for_view(b, v);
    if let Some(h) = lighting.sun.as_ref().and_then(|s| s.handle) {
        mapper::do_directional_light(b, HandleId::new(h), v);
    }
    for l in lighting.lights.iter_mut().filter(|l| l.shadows) {
        let Some(h) = l.handle else { continue };
        let index = mapper::find_or_create_shadow_maps(b, HandleId::new(h), v, camera.screen_size(l.position, l.radius));
        l.shadow_index = (index != INVALID).then_some(index);
    }
    mapper::upload_to_gpu(b);
    let attrs = shadow_attributes();
    mapper::set_shader_attributes(b, attrs);
    lighting.shadow_attributes = Some(attrs);
    lighting.shadow = attributes::constant_buffer(attrs, "DirectionalLightCB").and_then(|cb| managed_sun_shadow(&cb));
}

/// Cascade `n`'s map binds as `g_tUnderboxSunShadow` (cascade 0) or `g_tUnderboxSunShadow<n>`.
pub fn cascade_of(binding: &str) -> Option<usize> {
    match binding.strip_prefix("g_tUnderboxSunShadow")? {
        "" => Some(0),
        n => n.parse().ok().filter(|n| (1..4).contains(n)),
    }
}

/// The shadow map of the sun's cascade `cascade`: what `g_DirectionalLightShadowMapTextureIndex`
/// names for it in managed's `DirectionalLightCB` bytes, or `None` past the cascade count.
pub fn managed_cascade(cb: &[u8], cascade: usize) -> Option<texture::Ref> {
    let word = |i: usize| cb.get(i * 4..i * 4 + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    // Words 72..76: g_DirectionalLightShadowMapTextureIndex; 76: g_DirectionalLightCascadeCount.
    (cascade < word(76)? as usize).then_some(())?;
    crate::render::bindless::at(word(72 + cascade)?).map(|(t, _)| t)
}

/// Cascade 0 as a [`SunShadow`], from managed's `DirectionalLightCB` bytes.
pub fn managed_sun_shadow(cb: &[u8]) -> Option<SunShadow> {
    // Words 8..24: g_DirectionalLightWorldToShadowViewMatrices[0], world → uv and depth.
    let m: [f32; 16] = std::array::from_fn(|i| cb.get((8 + i) * 4..(9 + i) * 4).map_or(0.0, |w| f32::from_le_bytes([w[0], w[1], w[2], w[3]])));
    Some(SunShadow { world_to_shadow: Mat4::from_cols_array(&m), map: managed_cascade(cb, 0)?, reverse_z: true })
}
