//! The lights a view shades with: its worlds' `CSceneLightObject`s and the camera's ambient colour,
//! snapshotted once per view and handed to every draw of it. The first directional light is the
//! sun (`DirectionalLightCB`); point and spot lights are binned lights (`BinnedLightBufferV2`).
//! blocks.rs writes them into the constant blocks and buffers the shaders declare.
//!
//! A view with no lights and no ambient colour (a world without a light, unit tests) gets
//! [`Lighting::fallback`], the fixed light the stand-in shader used to hard-code, rather than black.
//! A 3D skybox's child view never does, and gets no ambient either: real lights it with its own
//! world's lights alone, so a skybox world without one draws black under its fog (corpus/outdoor
//! matches real within 0.2 per channel only so; the camera's ambient left a seam between faces).

mod blocks;
pub mod decals;
pub mod shadow;

pub use blocks::BLOCKS;

use crate::scene::light::{self, Type};
use crate::scene::{object, world};
use glam::{Quat, Vec3, Vec4};

/// lightbinner.hlsl's `LightType`.
pub mod kind {
    pub const POINT: u32 = 1;
    pub const DIRECTIONAL: u32 = 2;
    pub const SPOT: u32 = 3;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub kind: u32,
    pub position: Vec3,
    pub rotation: Quat,
    pub color: Vec3,
    pub radius: f32,
    /// Native's raw quadratic attenuation (managed's `QuadraticAttenuation` / 10000).
    pub quadratic: f32,
    /// Spot cone half-angles (inner, outer) in degrees.
    pub cone: Option<(f32, f32)>,
    pub fog: f32,
    pub shadows: bool,
    /// The scene object's managed handle id, for managed's shadow mapper.
    pub handle: Option<i32>,
    /// `ShadowMapIndex`: managed's `ProjectedShadows` (spot) or `ProjectedCubeShadows` (point) index.
    pub shadow_index: Option<u32>,
}

impl Light {
    /// The way the light shines (its +X).
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::X
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lighting {
    /// `AmbientLightColor`: rgb, and w the IBL/flat lerp.
    pub ambient: Vec4,
    pub sun: Option<Light>,
    pub lights: Vec<Light>,
    /// The sun's first cascade, once managed's shadow mapper has drawn it for the view (shadow.rs).
    pub shadow: Option<shadow::SunShadow>,
    /// What the shadow mapper set for the view (`DirectionalLightCB`, `ProjectedShadows`,
    /// `ProjectedCubeShadows`): every draw of the view binds from it.
    pub shadow_attributes: Option<crate::render::attributes::Ref>,
    /// The view's exposure (`g_flToneMapScalarLinear`, render/exposure.rs); none is 1.
    pub tone_map: Option<f32>,
    /// The view's analytic fog (`FogConstants`, render/fog.rs), when managed set one.
    pub fog: Option<std::sync::Arc<[u8]>>,
    /// The view's gradient, cubemap and volumetric settings (render/fog.rs).
    pub view_fog: Option<std::sync::Arc<crate::render::fog::ViewFog>>,
    /// The view's rendered froxels, when any volume shows (render/fog_volume.rs).
    pub fog_volume: Option<std::sync::Arc<crate::render::fog_volume::Frustum>>,
    /// The worlds' clustered decals (decals.rs).
    pub decals: Option<std::sync::Arc<decals::Decals>>,
}

impl Lighting {
    /// The stand-in's former fixed light: a white-ish sun from (0.4, 0.3, 0.85) and 0.3 ambient.
    pub fn fallback() -> Lighting {
        let toward = Vec3::new(0.4, 0.3, 0.85).normalize();
        let rotation = Quat::from_rotation_arc(Vec3::X, -toward);
        let sun = Light { kind: kind::DIRECTIONAL, position: Vec3::ZERO, rotation, color: Vec3::splat(0.7), radius: 0.0, quadratic: 0.0, cone: None, fog: 1.0, shadows: false, handle: None, shadow_index: None };
        Lighting { ambient: Vec4::new(0.3, 0.3, 0.3, 1.0), sun: Some(sun), ..Default::default() }
    }

    fn is_dark(&self) -> bool {
        self.sun.is_none() && self.lights.is_empty() && self.ambient.truncate() == Vec3::ZERO
    }

    /// [`Lighting::of_worlds`], or [`Lighting::fallback`] when that leaves the view dark.
    pub fn collect(worlds: &[world::Ref], ambient: Vec4) -> Lighting {
        let out = Lighting::of_worlds(worlds, ambient);
        if out.is_dark() { Lighting::fallback() } else { out }
    }

    /// The rendering lights of `worlds` and the camera's `ambient` (managed's `ambientColor`).
    pub fn of_worlds(worlds: &[world::Ref], ambient: Vec4) -> Lighting {
        let mut out = Lighting { ambient, ..Default::default() };
        let mut found = Vec::new();
        for (r, obj) in object::refs_in_worlds(worlds) {
            let (rendering, p) = {
                let s = obj.state.lock().unwrap();
                (s.rendering, s.transform.position.vec)
            };
            if rendering && obj.kind == crate::handles::Kind::DecalSceneObject {
                found.push(decals::Decal::read(r.as_raw().cast(), &obj));
                continue;
            }
            let Some(l) = obj.light.as_ref().filter(|_| rendering) else { continue };
            let l = Light { handle: Some(obj.handle.id::<()>().id).filter(|id| *id >= 0), ..from_scene(&l.lock().unwrap(), Vec3::new(p.x, p.y, p.z)) };
            match l.kind {
                kind::DIRECTIONAL if out.sun.is_none() => out.sun = Some(l),
                _ => out.lights.push(l),
            }
        }
        if !found.is_empty() {
            out.decals = Some(std::sync::Arc::new(decals::Decals::pack(found)));
        }
        out
    }
}

fn from_scene(l: &light::Light, position: Vec3) -> Light {
    let q = l.direction.quat;
    let rotation = Quat::from_xyzw(q.x, q.y, q.z, q.w);
    let rotation = if rotation.length_squared() > 0.0 { rotation.normalize() } else { Quat::IDENTITY };
    let kind = match l.kind {
        Type::Point => kind::POINT,
        Type::Spot | Type::Ortho => kind::SPOT,
        Type::Directional => kind::DIRECTIONAL,
    };
    let c = l.color.vec;
    Light {
        kind,
        position,
        rotation,
        color: Vec3::new(c.x, c.y, c.z),
        radius: l.radius,
        quadratic: l.quadratic_attn,
        cone: (kind == kind::SPOT).then_some((l.theta, l.phi)),
        fog: l.fog_strength,
        shadows: l.shadows,
        handle: None,
        shadow_index: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_without_lights_gets_the_fallback() {
        assert_eq!(Lighting::collect(&[], Vec4::ZERO), Lighting::fallback());
        let ambient = Vec4::new(0.1, 0.2, 0.3, 1.0);
        assert_eq!(Lighting::collect(&[], ambient), Lighting { ambient, ..Default::default() });
        assert_eq!(Lighting::of_worlds(&[], Vec4::ZERO), Lighting::default(), "a skybox stays unlit");
    }

    #[test]
    fn fallback_sun_shines_down_the_old_fixed_light() {
        let sun = Lighting::fallback().sun.unwrap();
        assert!((-sun.forward()).distance(Vec3::new(0.4, 0.3, 0.85).normalize()) < 1e-5);
    }
}
