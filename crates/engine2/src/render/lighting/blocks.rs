//! A view's [`Lighting`] as the core shaders' lighting blocks hold it: `ViewLightingConfigV2` and
//! `BinnedLightBufferV2` (common/lightbinner.hlsl), `DirectionalLightCB`
//! (Shadows/DirectionalLightShadow.hlsl) and the clustered-culling inputs
//! (common/classes/ClusterCulling.hlsl). There is no culling: every light sits in one cluster.
//!
//! Distance falloff is our own packing (native's isn't public): distances are taken relative to
//! the radius (`LinearFalloff = l / r`, `QuadraticFalloff = q / r²`) and `FalloffBias` is the
//! curve's value at the radius, so `saturate(1 / (l·d + q·d²) - bias)` reaches 0 there.

use super::{Light, Lighting};
use crate::render::shading::layout::{Block, Value};
use glam::{Mat4, Vec4};

/// Uniform blocks this module fills.
pub const BLOCKS: [&str; 3] = ["ViewLightingConfigV2", "DirectionalLightCB", "ClusteredLightingConstants"];

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
        let r = self.radius.max(1e-3);
        let total = self.linear + self.quadratic;
        let bias = if total > 0.0 { 1.0 / total } else { 0.0 };
        (self.linear / r, self.quadratic / (r * r), bias)
    }

    /// `SpotLightInnerOuterConeCosines`; a point light's never rejects and saturates to 1.
    fn cone_cosines(&self) -> Vec4 {
        let Some((inner, outer)) = self.cone else { return Vec4::new(0.0, -2.0, 1.0, 0.0) };
        let outer = outer.clamp(0.01, 89.9).to_radians();
        let inner = inner.to_radians().clamp(0.0, outer);
        let (ci, co) = (inner.cos(), outer.cos());
        Vec4::new(ci, co, 1.0 / (ci - co).max(1e-4), outer.tan())
    }

    fn binned(&self, name: &str) -> Option<Value> {
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
            "ShadowMapIndex" | "LightCookieTextureIndex" | "ShadowMaskTextureIndex" => s(u32::MAX as f32),
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
        match name {
            "g_DirectionalLightColor" => v(c.x, c.y, c.z, sun.fog),
            "g_DirectionalLightDirection" => v(d.x, d.y, d.z, 0.0),
            "g_DirectionalLightEnabled" => s(1.0),
            _ => None,
        }
    }

    /// A structured buffer's bytes for elements laid out as `element`; `None` for buffers this
    /// module doesn't own.
    pub fn storage(&self, name: &str, element: &Block) -> Option<Vec<u8>> {
        let n = self.lights.len();
        Some(match name {
            "BinnedLightBufferV2" => element.fill_elements(n, |i, f| self.lights[i].binned(f)),
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

    fn point(radius: f32) -> Light {
        Light { kind: kind::POINT, position: Vec3::new(1.0, 2.0, 3.0), rotation: Quat::IDENTITY, color: Vec3::ONE, radius, linear: 0.0, quadratic: 1.0, cone: None, fog: 1.0, shadows: false }
    }

    #[test]
    fn falloff_reaches_zero_at_the_radius() {
        let (l, q, bias) = point(200.0).falloff();
        let at = |d: f32| (1.0 / (l * d + q * d * d) - bias).clamp(0.0, 1.0);
        assert_eq!((at(10.0), at(200.0)), (1.0, 0.0));
        assert!(at(180.0) > 0.0);
    }

    #[test]
    fn light_to_world_rows_are_direction_then_position() {
        let mut l = point(1.0);
        l.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let m = l.light_to_world();
        assert!(m.row(0).truncate().distance(Vec3::Y) < 1e-6 && m.row(3).truncate() == l.position);
    }
}
