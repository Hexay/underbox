//! A view's [`Lighting`] as the core shaders' lighting blocks hold it: `ViewLightingConfigV2` and
//! `BinnedLightBufferV2` (common/lightbinner.hlsl), `DirectionalLightCB`
//! (Shadows/DirectionalLightShadow.hlsl) and the clustered-culling inputs
//! (common/classes/ClusterCulling.hlsl). There is no culling: every light sits in one cluster.
//!
//! Distance falloff is packed as `CLightBinnerStandard::AddLight` does (managed's mirror:
//! `LightBinnerFeature.Pack`): no linear term, the raw quadratic attenuation over world units, and
//! `FalloffBias = 1 / (r²·q)` so `saturate(1 / (q·d²) - bias)` reaches 0 at the radius. Map lights
//! carry Hammer's colour × 10000 with q = 1, so 1 at 100 units; components carry q = 1e-4.

use super::{Light, Lighting, local_shadow, shadow};
use crate::render::shading::layout::{Block, Value};
use crate::render::texture;
use glam::{Mat4, Vec4};

/// Uniform blocks this module fills.
pub const BLOCKS: [&str; 4] = ["ViewLightingConfigV2", "DirectionalLightCB", "ClusteredLightingConstants", local_shadow::BLOCK];

fn v(x: f32, y: f32, z: f32, w: f32) -> Option<Value> {
    Some(Value::Vec(Vec4::new(x, y, z, w)))
}

fn s(x: f32) -> Option<Value> {
    v(x, 0.0, 0.0, 0.0)
}

impl Light {
    /// `LightToWorld` as the shader indexes it: rows forward, left, up, position.
    fn light_to_world(&self) -> Mat4 {
        let r = Mat4::from_quat(self.rotation);
        Mat4::from_cols(r.x_axis, r.y_axis, r.z_axis, self.position.extend(1.0)).transpose()
    }

    /// (linear, quadratic, bias) for `CalculateDistanceFalloff`; see the module docs.
    fn falloff(&self) -> (f32, f32, f32) {
        let q = self.quadratic;
        (0.0, q, 1.0 / (f32::EPSILON + self.radius * self.radius * q))
    }

    /// `SpotLightInnerOuterConeCosines`; a point light's never rejects and saturates to 1.
    fn cone_cosines(&self) -> Vec4 {
        let Some((inner, outer)) = self.cone else { return Vec4::new(0.0, -2.0, 1.0, 0.0) };
        let outer = outer.clamp(0.01, 89.9).to_radians();
        let inner = inner.to_radians().clamp(0.0, outer);
        let (ci, co) = (inner.cos(), outer.cos());
        Vec4::new(ci, co, 1.0 / (ci - co).max(1e-4), outer.tan())
    }

    /// A `BinnedLightBufferV2` member; `shadow` is the light's first local shadow tile.
    fn binned(&self, name: &str, shadow: Option<u32>) -> Option<Value> {
        let (linear, quadratic, bias) = self.falloff();
        match name {
            "Type" => s(self.kind as f32),
            // Visible, diffuse and specular.
            "Flags" => s(0x7 as f32),
            "LightToWorld" => Some(Value::Mat(self.light_to_world())),
            "Color" => v(self.color.x, self.color.y, self.color.z, 0.0),
            "LinearFalloff" => s(linear),
            "QuadraticFalloff" => s(quadratic),
            "FalloffBias" => s(bias),
            "Radius" => s(self.radius),
            "RadiusSquared" => s(self.radius * self.radius),
            "SpotLightInnerOuterConeCosines" => Some(Value::Vec(self.cone_cosines())),
            "FogIntensity" => s(self.fog),
            "ShadowMapIndex" => s(shadow.map_or(u32::MAX, |t| local_shadow::INDEX + t) as f32),
            "LightCookieTextureIndex" | "ShadowMaskTextureIndex" => s(u32::MAX as f32),
            _ => None,
        }
    }
}

impl Lighting {
    /// A member of one of [`BLOCKS`]; `None` for other blocks and members.
    pub fn constant(&self, block: &str, name: &str) -> Option<Value> {
        let n = self.lights.len() as f32;
        match (block, name) {
            ("ViewLightingConfigV2", "NumLights") => v(n, 0.0, 0.0, 0.0),
            ("ViewLightingConfigV2", "AmbientLightColor") => Some(Value::Vec(self.ambient)),
            ("DirectionalLightCB", _) => self.sun_constant(name),
            (local_shadow::BLOCK, local_shadow::MATRICES) => self.local.as_ref().map(|l| Value::Array(l.tiles.iter().copied().map(Value::Mat).collect())),
            ("ClusteredLightingConstants", "ClusterCounts") => v(1.0, 1.0, 1.0, 1.0),
            ("ClusteredLightingConstants", "ClusterInvCounts") => v(1.0, 1.0, 1.0, 0.0),
            ("ClusteredLightingConstants", "ClusterZParams") => v(1.0, 0.0, 1e-3, 1e9),
            ("ClusteredLightingConstants", "ClusterScreenParams") => v(1.0, 1.0, 1.0, 1.0),
            ("ClusteredLightingConstants", "ClusterCapacitiesVec") => v(n.max(1.0), 0.0, 0.0, 0.0),
            _ => None,
        }
    }

    fn sun_constant(&self, name: &str) -> Option<Value> {
        let sun = self.sun.as_ref()?;
        let (c, d) = (sun.color, sun.forward());
        let shadow = self.shadow.as_ref();
        match name {
            "g_DirectionalLightColor" => v(c.x, c.y, c.z, sun.fog),
            "g_DirectionalLightDirection" => v(d.x, d.y, d.z, 0.0),
            "g_DirectionalLightEnabled" => s(1.0),
            "g_DirectionalLightCascadeCount" => s(shadow.map_or(0.0, |_| 1.0)),
            "g_DirectionalLightWorldToShadowViewMatrices" => shadow.map(|sh| Value::Array(vec![Value::Mat(sh.world_to_shadow)])),
            "g_DirectionalLightInverseShadowMapSize" => s(1.0 / shadow::SIZE as f32),
            "g_DirectionalLightShadowMapTextureIndex" => shadow.and(v(shadow::INDEX as f32, 0.0, 0.0, 0.0)),
            _ => None,
        }
    }

    /// A texture the lighting supplies by shader name (the shadow maps).
    pub fn texture(&self, name: &str) -> Option<texture::Ref> {
        match name {
            shadow::TEXTURE => self.shadow.as_ref().map(|s| s.map),
            local_shadow::TEXTURE => self.local.as_ref().map(|l| l.map),
            _ => None,
        }
    }

    /// A structured buffer's bytes for elements laid out as `element`; `None` for buffers this
    /// module doesn't own.
    pub fn storage(&self, name: &str, element: &Block) -> Option<Vec<u8>> {
        let n = self.lights.len();
        Some(match name {
            "BinnedLightBufferV2" => {
                let first = |i: usize| self.local.as_ref().and_then(|l| l.first.get(i).copied().flatten());
                element.fill_elements(n, |i, f| self.lights[i].binned(f, first(i)))
            }
            "ClusterLightCounts" => element.fill_elements(1, |_, _| s(n as f32)),
            "ClusterLightIndices" => element.fill_elements(n, |i, _| s(i as f32)),
            "ClusterEnvMapCounts" | "ClusterDecalCounts" | "ClusterEnvMapIndices" | "ClusterDecalIndices" => element.fill_elements(1, |_, _| None),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::lighting::kind;
    use glam::{Quat, Vec3};

    /// A `PointLight` component's light: `Attenuation` 1 reaches native as 1e-4.
    fn point(radius: f32) -> Light {
        Light { kind: kind::POINT, position: Vec3::new(1.0, 2.0, 3.0), rotation: Quat::IDENTITY, color: Vec3::ONE, radius, quadratic: 1e-4, cone: None, fog: 1.0, shadows: false }
    }

    /// The stand-in's `saturate(1 / (l·d + q·d²) - bias)` at distance `d`.
    fn falloff_at(light: &Light, d: f32) -> f32 {
        let (l, q, bias) = light.falloff();
        (1.0 / (l * d + q * d * d) - bias).clamp(0.0, 1.0)
    }

    #[test]
    fn falloff_reaches_zero_at_the_radius() {
        let l = point(200.0);
        assert_eq!((falloff_at(&l, 10.0), falloff_at(&l, 200.0)), (1.0, 0.0));
        assert!(falloff_at(&l, 180.0) > 0.0);
    }

    /// corpus/mapworld's `light_omni` (brightness 2, range 512, attenuation2 1): managed hands
    /// native colour × brightness × 10000 and the raw quadratic 1, so 120 units under it the
    /// light is about 1.4× its colour, not 10⁴× (the blown-out map frame).
    #[test]
    fn hammer_light_is_inverse_square_from_100_units() {
        let lamp = Light { quadratic: 1.0, color: Vec3::splat(2.0 * 10000.0), ..point(512.0) };
        let lit = |d: f32| lamp.color.x * falloff_at(&lamp, d);
        assert!((lit(100.0) - 2.0).abs() < 0.2, "{}", lit(100.0));
        assert!((1.2..1.4).contains(&lit(120.0)), "{}", lit(120.0));
        assert!(lit(400.0) < 0.1 && lit(512.0) == 0.0);
    }

    #[test]
    fn light_to_world_rows_are_direction_then_position() {
        let mut l = point(1.0);
        l.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let m = l.light_to_world();
        assert!(m.row(0).truncate().distance(Vec3::Y) < 1e-6 && m.row(3).truncate() == l.position);
    }
}
