//! The sun's shadow map: one cascade, an orthographic depth render from the sun over the sphere
//! that bounds the camera frustum up to [`DISTANCE`] away, drawn before a view's layers when the
//! sun casts shadows. The stand-in samples it as `g_tUnderboxSunShadow` through
//! `g_DirectionalLightWorldToShadowViewMatrices[0]` (world → texture uv and depth). Real shaders
//! find cascades by bindless index, which the binder can't serve yet.

use super::Lighting;
use crate::render::draw::{self, Target, model as model_draw};
use crate::render::texture;
use crate::scene::{object, world};
use glam::camera::rh;
use glam::{Mat4, Vec3, Vec4};
use render::pass::Load;
use std::sync::OnceLock;

/// Shadow map resolution (square).
pub const SIZE: u32 = 2048;
/// How far from the camera the cascade reaches.
const DISTANCE: f32 = 2000.0;
/// The texture the stand-in samples.
pub const TEXTURE: &str = "g_tUnderboxSunShadow";
/// `g_DirectionalLightShadowMapTextureIndex.x` naming [`TEXTURE`] (the stand-in's
/// `UNDERBOX_SUN_SHADOW_INDEX`); managed's renderer fills in bindless indices instead.
pub const INDEX: u32 = 0x7FFF_FF00;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunShadow {
    /// World → (u, v, depth) in the shadow map.
    pub world_to_shadow: Mat4,
    pub map: texture::Ref,
}

/// One map for the process: views render one at a time and each redraws it before its layers.
fn map() -> texture::Ref {
    static MAP: OnceLock<texture::Ref> = OnceLock::new();
    // Image format 62: D32F.
    *MAP.get_or_init(|| texture::create_target("sun shadow", SIZE, SIZE, 62))
}

/// The sphere around the part of the frustum `view_proj` sees up to `DISTANCE` from its near plane.
fn bounds(view_proj: Mat4) -> (Vec3, f32) {
    let inv = view_proj.inverse();
    let at = |x: f32, y: f32, z: f32| inv.project_point3(Vec3::new(x, y, z));
    let mut corners = Vec::with_capacity(8);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let (near, far) = (at(x, y, 0.0), at(x, y, 1.0));
        let ray = far - near;
        corners.push(near);
        corners.push(near + ray * (DISTANCE / ray.length().max(1e-3)).min(1.0));
    }
    let center = corners.iter().sum::<Vec3>() / 8.0;
    (center, corners.iter().map(|c| c.distance(center)).fold(0.0, f32::max).max(1.0))
}

/// The sun's view-projection over the sphere, with its centre snapped to whole texels so the map
/// doesn't shimmer as the camera moves.
fn light_view_proj(direction: Vec3, center: Vec3, radius: f32) -> Mat4 {
    let up = if direction.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
    let back = radius + DISTANCE;
    let view = rh::view::look_at_mat4(-direction * back, Vec3::ZERO, up);
    let texel = 2.0 * radius / SIZE as f32;
    let c = view.transform_point3(center);
    let snapped = Vec3::new((c.x / texel).round() * texel, (c.y / texel).round() * texel, c.z);
    let proj = rh::proj::directx::orthographic(snapped.x - radius, snapped.x + radius, snapped.y - radius, snapped.y + radius, -c.z - radius - back, -c.z + radius);
    proj * view
}

/// Clip space → shadow-map uv (y down) and depth.
fn to_texture() -> Mat4 {
    Mat4::from_cols(Vec4::new(0.5, 0.0, 0.0, 0.0), Vec4::new(0.0, -0.5, 0.0, 0.0), Vec4::Z, Vec4::new(0.5, 0.5, 0.0, 1.0))
}

/// Render the sun's shadow map for a view through `view_proj` when the sun casts shadows, and
/// record it on `lighting`.
pub fn render(lighting: &mut Lighting, worlds: &[world::Ref], view_proj: Mat4) {
    let Some(sun) = lighting.sun.as_ref().filter(|s| s.shadows) else { return };
    let (center, radius) = bounds(view_proj);
    let light = light_view_proj(sun.forward(), center, radius);
    let mut draws = Vec::new();
    for obj in object::in_worlds(worlds) {
        let s = obj.state.lock().unwrap();
        if s.rendering && obj.light.is_none() {
            draws.extend(model_draw::object(&s, light).into_iter().map(|d| draw::Draw { shaded: None, ..d }));
        }
    }
    let target = Target { depth: map(), ..Default::default() };
    draw::execute(&target, lighting, Load { color: None, depth: Some(1.0) }, &draws);
    lighting.shadow = Some(SunShadow { world_to_shadow: to_texture() * light, map: target.depth });
}

/// The shadow map of managed's first sun cascade: what `g_DirectionalLightShadowMapTextureIndex.x`
/// names in its `DirectionalLightCB` bytes, or `None` without cascades. Managed's maps are reverse-Z.
// TODO: in a client boot managed's cascade casts no shadow though map, matrix and binding check
// out (see docs/lighting.md "Managed sun shadows").
pub fn managed_cascade(cb: &[u8]) -> Option<texture::Ref> {
    let word = |i: usize| cb.get(i * 4..i * 4 + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    // Word 72: g_DirectionalLightShadowMapTextureIndex.x; 76: g_DirectionalLightCascadeCount.
    (word(76)? > 0).then_some(())?;
    crate::render::tools::texture_at_view_index(word(72)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_projection_contains_the_view_sphere() {
        let view_proj = rh::proj::directx::perspective(1.2, 1.5, 10.0, 10000.0) * rh::view::look_at_mat4(Vec3::new(-200.0, 0.0, 100.0), Vec3::ZERO, Vec3::Z);
        let (center, radius) = bounds(view_proj);
        assert!(radius < DISTANCE * 1.5);
        let m = to_texture() * light_view_proj(Vec3::new(0.3, 0.2, -0.9).normalize(), center, radius);
        for p in [center, center + Vec3::X * radius * 0.7, center - Vec3::Z * radius * 0.7] {
            let t = m.project_point3(p);
            assert!((0.0..=1.0).contains(&t.x) && (0.0..=1.0).contains(&t.y) && (0.0..=1.0).contains(&t.z), "{p} → {t}");
        }
    }
}
