//! A view's [`Lighting`] as the core shaders' lighting blocks hold it: `ViewLightingConfigV2` and
//! `BinnedLightBufferV2` (common/lightbinner.hlsl), `DirectionalLightCB`
//! (Shadows/DirectionalLightShadow.hlsl) and the clustered-culling inputs
//! (common/classes/ClusterCulling.hlsl). There is no culling: every light sits in one cluster
//! (see [`Lighting::constant`]).
//!
//! Distance falloff is packed as `CLightBinnerStandard::AddLight` does (managed's mirror:
//! `LightBinnerFeature.Pack`): no linear term, the raw quadratic attenuation over world units, and
//! `FalloffBias = 1 / (r²·q)` so `saturate(1 / (q·d²) - bias)` reaches 0 at the radius. Map lights
//! carry Hammer's colour × 10000 with q = 1, so 1 at 100 units; components carry q = 1e-4.

use super::{Light, Lighting, decals};
use crate::render::shading::layout::{Block, Value};
use crate::render::shading::view::View;
use crate::render::texture;
use glam::{Vec3, Vec4};

/// Uniform blocks this module fills. `DirectionalLightCB` here is the unshadowed sun; with
/// shadows, managed's shadow mapper supplies the whole block (shadow.rs).
pub const BLOCKS: [&str; 3] = ["ViewLightingConfigV2", "DirectionalLightCB", "ClusteredLightingConstants"];

fn v(x: f32, y: f32, z: f32, w: f32) -> Option<Value> {
    Some(Value::Vec(Vec4::new(x, y, z, w)))
}

fn s(x: f32) -> Option<Value> {
    v(x, 0.0, 0.0, 0.0)
}

/// Words in native's `GPULight` (lightbinner_standard.h): 156 bytes in scalar layout, which
/// managed's `LightBinnerFeature.GpuLight` uploads too. std430 would put `LightToWorld` at 16, not 12.
pub const GPU_LIGHT_WORDS: usize = 39;

impl Light {
    /// `LightToWorld`'s rows in memory: columns forward, left, up, position. Read column-major,
    /// the shader's rows 0 and 3 are forward and position. A point light's axes stay zero, as
    /// native packs it: its cone test then reads `0 - (-1)`, so the cone factor is exactly 1.
    fn light_to_world_rows(&self) -> [Vec4; 4] {
        let axes = |v: Vec3| if self.cone.is_some() { self.rotation * v } else { Vec3::ZERO };
        let (f, l, u, p) = (axes(Vec3::X), axes(Vec3::Y), axes(Vec3::Z), self.position);
        [Vec4::new(f.x, l.x, u.x, p.x), Vec4::new(f.y, l.y, u.y, p.y), Vec4::new(f.z, l.z, u.z, p.z), Vec4::W]
    }

    /// (linear, quadratic, bias) for `CalculateDistanceFalloff`; see the module docs.
    fn falloff(&self) -> (f32, f32, f32) {
        let q = self.quadratic;
        (0.0, q, 1.0 / (f32::EPSILON + self.radius * self.radius * q))
    }

    /// `SpotLightInnerOuterConeCosines`; a point light's is native's (0, -1, 1, 0).
    fn cone_cosines(&self) -> Vec4 {
        let Some((inner, outer)) = self.cone else { return Vec4::new(0.0, -1.0, 1.0, 0.0) };
        let outer = outer.clamp(0.01, 89.9).to_radians();
        let inner = inner.to_radians().clamp(0.0, outer);
        let (ci, co) = (inner.cos(), outer.cos());
        Vec4::new(ci, co, 1.0 / (ci - co).max(0.01), outer.tan())
    }

    /// The light as one `GPULight`.
    fn gpu_light(&self) -> [u32; GPU_LIGHT_WORDS] {
        let (linear, quadratic, bias) = self.falloff();
        let mut w = [0u32; GPU_LIGHT_WORDS];
        // Type, shape (sphere), flags (visible, diffuse, specular).
        w[..3].copy_from_slice(&[self.kind, 0, 0x7]);
        for (i, row) in self.light_to_world_rows().iter().enumerate() {
            w[3 + i * 4..7 + i * 4].copy_from_slice(&row.to_array().map(f32::to_bits));
        }
        let floats = [self.color.x, self.color.y, self.color.z, linear, quadratic, bias, self.radius, self.radius * self.radius, 0.0, 0.0];
        w[19..29].copy_from_slice(&floats.map(f32::to_bits));
        w[29..33].copy_from_slice(&self.cone_cosines().to_array().map(f32::to_bits));
        w[33] = self.fog.to_bits();
        // Shadow map, cookie and shadow mask indices; modulator offset and count stay 0.
        w[34..37].copy_from_slice(&[self.shadow_index.unwrap_or(u32::MAX), u32::MAX, u32::MAX]);
        w
    }
}

impl Lighting {
    /// A member of one of [`BLOCKS`] for a draw through `view`; `None` for other blocks and
    /// members. The cluster grid is one cluster deep over the view's depth range; managed's
    /// `ClusteredCullingLayer` binds its own grid on the view's attributes, which wins.
    pub fn constant(&self, block: &str, name: &str, view: &View) -> Option<Value> {
        let n = self.lights.len() as f32;
        let decals = self.decal_count() as f32;
        let (near, far) = view.near_far();
        let [_, _, w, h] = view.viewport.map(|x| x.max(1.0));
        match (block, name) {
            ("ViewLightingConfigV2", "NumLights") => v(n, 0.0, 0.0, decals),
            ("ViewLightingConfigV2", "AmbientLightColor") => Some(Value::Vec(self.ambient)),
            ("DirectionalLightCB", _) => self.sun_constant(name),
            ("ClusteredLightingConstants", "ClusterCounts") => v(1.0, 1.0, 1.0, 1.0),
            ("ClusteredLightingConstants", "ClusterInvCounts") => v(1.0, 1.0, 1.0, 0.0),
            ("ClusteredLightingConstants", "ClusterZParams") => {
                let scale = 1.0 / (far / near).ln();
                v(scale, -near.ln() * scale, near, far)
            }
            ("ClusteredLightingConstants", "ClusterScreenParams") => v(w, h, 1.0 / w, 1.0 / h),
            ("ClusteredLightingConstants", "ClusterCapacitiesVec") => v(n.max(1.0), 0.0, decals, 0.0),
            _ => None,
        }
    }

    fn sun_constant(&self, name: &str) -> Option<Value> {
        let sun = self.sun.as_ref()?;
        let (c, d) = (sun.color, sun.forward());
        match name {
            "g_DirectionalLightColor" => v(c.x, c.y, c.z, sun.fog),
            "g_DirectionalLightDirection" => v(d.x, d.y, d.z, 0.0),
            "g_DirectionalLightEnabled" => s(1.0),
            _ => None,
        }
    }

    /// A texture the lighting supplies by shader name.
    pub fn texture(&self, name: &str) -> Option<texture::Ref> {
        match name {
            "g_fogCubemapTexture" => crate::render::fog::cube_texture(self),
            _ => None,
        }
    }

    /// A structured buffer's bytes for elements laid out as `element`; `None` for buffers this
    /// module doesn't own.
    pub fn storage(&self, name: &str, element: &Block) -> Option<Vec<u8>> {
        let n = self.lights.len();
        Some(match name {
            "BinnedLightBufferV2" => self.lights.iter().flat_map(Light::gpu_light).flat_map(u32::to_le_bytes).collect(),
            "ClusterLightCounts" => element.fill_elements(1, |_, _| s(n as f32)),
            "ClusterLightIndices" => element.fill_elements(n, |i, _| s(i as f32)),
            "ClusterDecalCounts" => element.fill_elements(1, |_, _| s(self.decal_count() as f32)),
            "ClusterDecalIndices" => element.fill_elements(self.decal_count().max(1), |i, _| s(i as f32)),
            "DecalsBuffer" => decals::Decals::bytes(self.decals.as_ref().map_or(&[][..], |d| &d.gpu)),
            "DecalsExtraDataBuffer" => decals::Decals::bytes(self.decals.as_ref().map_or(&[][..], |d| &d.extra)),
            "ClusterEnvMapCounts" | "ClusterEnvMapIndices" => element.fill_elements(1, |_, _| None),
            _ => return None,
        })
    }

    fn decal_count(&self) -> usize {
        self.decals.as_ref().map_or(0, |d| d.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::lighting::kind;
    use glam::{Quat, Vec3};

    /// A `PointLight` component's light: `Attenuation` 1 reaches native as 1e-4.
    fn point(radius: f32) -> Light {
        Light { kind: kind::POINT, position: Vec3::new(1.0, 2.0, 3.0), rotation: Quat::IDENTITY, color: Vec3::ONE, radius, quadratic: 1e-4, cone: None, fog: 1.0, shadows: false, handle: None, shadow_index: None }
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

    /// Word offsets managed's `GpuLight` has (scalar layout): forward and position down the
    /// matrix columns from word 3, colour at 19, radius² at 26, shadow map index at 34.
    #[test]
    fn gpu_light_matches_managed_scalar_layout() {
        let mut l = Light { kind: kind::SPOT, cone: Some((15.0, 30.0)), ..point(5.0) };
        l.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        l.shadow_index = Some(2);
        let w = l.gpu_light().map(f32::from_bits);
        let forward = Vec3::new(w[3], w[7], w[11]);
        assert!(forward.distance(Vec3::Y) < 1e-6, "{forward}");
        let p = point(5.0).gpu_light().map(f32::from_bits);
        assert_eq!(([p[3], p[7], p[11]], [p[29], p[30], p[31]]), ([0.0; 3], [0.0, -1.0, 1.0]));
        assert_eq!([w[6], w[10], w[14], w[18]], [1.0, 2.0, 3.0, 1.0]);
        assert_eq!([w[19], w[26]], [1.0, 25.0]);
        assert_eq!(l.gpu_light()[34], 2);
        assert_eq!(point(5.0).gpu_light()[34], u32::MAX);
    }
}
