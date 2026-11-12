//! `PerViewConstantBuffer_t` (core `common.fxc`): the per-view constants a translated program
//! reads, from the draw's reverse-Z world → clip matrix ([`Space`]) and its target, laid out as
//! real engine2's capture shows them (docs/differential-gpu.md). `g_flTime` and
//! `g_vRandomFloats` stay 0 so frames are deterministic.

use super::layout::Value;
use super::view_space::{Space, forward_z};
use glam::{Mat4, Vec4};

/// The view a draw renders through.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub view_proj: Mat4,
    /// `x, y, w, h` in pixels.
    pub viewport: [f32; 4],
    /// Render target size in pixels.
    pub target: [f32; 2],
    /// `g_flToneMapScalarLinear`, the view's exposure.
    pub tone_map: f32,
    /// The viewport's depth range, `g_flViewportMinZ` / `g_flViewportMaxZ`.
    pub depth_range: [f32; 2],
}

impl Default for View {
    fn default() -> View {
        View { view_proj: Mat4::IDENTITY, viewport: [0.0; 4], target: [1.0; 2], tone_map: 1.0, depth_range: [0.0, 1.0] }
    }
}

/// The block's binding name, as DXC names the cbuffer.
pub const BLOCK: &str = "PerViewConstantBuffer_t";

/// Native's morph target atlas size.
const MORPH_ATLAS: [f32; 2] = [2048.0, 1024.0];

impl View {
    pub fn with_view_proj(self, view_proj: Mat4) -> View {
        View { view_proj, ..self }
    }

    pub fn space(&self) -> Space {
        Space::of(self.view_proj)
    }

    /// The camera's world position, w 1.
    pub fn camera_position(&self) -> Vec4 {
        self.space().position.extend(1.0)
    }

    /// The camera's forward and up directions in world space, w 0.
    pub fn camera_axes(&self) -> (Vec4, Vec4) {
        let s = self.space();
        (s.forward.extend(0.0), s.up.extend(0.0))
    }

    /// The projection's near and far planes.
    pub fn near_far(&self) -> (f32, f32) {
        let s = self.space();
        (s.near.max(1e-4), s.far.max(s.near + 1e-2))
    }

    /// A global's `Source( ... )` value. `Viewport` is the raster viewport in pixels, which the UI
    /// shaders map screen positions through.
    // TODO: TextureDim / InvTextureDim need the texture the variable is paired with.
    pub fn source(&self, source: &str) -> Option<Vec4> {
        (source == "Viewport").then(|| Vec4::from_array(self.viewport))
    }

    /// Clip → pixels of the viewport (y down), as `g_matViewToScreen` applies after the projection.
    fn screen_from_clip(&self) -> Mat4 {
        let [x, y, w, h] = self.viewport;
        Mat4::from_cols(Vec4::new(w / 2.0, 0.0, 0.0, 0.0), Vec4::new(0.0, -h / 2.0, 0.0, 0.0), Vec4::Z, Vec4::new(x + w / 2.0, y + h / 2.0, 0.0, 1.0))
    }

    /// The block's members by name, taking the view's [`Space`] apart once.
    pub fn constants(&self) -> impl Fn(&str) -> Option<Value> + '_ {
        let s = self.space();
        move |name| self.constant(&s, name)
    }

    fn constant(&self, s: &Space, name: &str) -> Option<Value> {
        let [x, y, w, h] = self.viewport;
        let [tw, th] = self.target;
        let (w, h, tw, th) = (w.max(1.0), h.max(1.0), tw.max(1.0), th.max(1.0));
        let v = |x: f32, y: f32, z: f32, w: f32| Some(Value::Vec(Vec4::new(x, y, z, w)));
        let pos = s.position;
        let m = |m: Mat4| Some(Value::Mat(m));
        let pair = |v: Value| Some(Value::Array(vec![v.clone(), v]));
        match name {
            "g_matWorldToProjection" => m(s.camera_relative()),
            "g_matProjectionToWorld" => m(s.camera_relative().inverse()),
            "g_matWorldToView" => m(s.rotation * Mat4::from_translation(-pos)),
            // Native stores this one transposed.
            "g_matViewToProjection" => m(s.projection.transpose()),
            "g_vInvProjRow3" => Some(Value::Vec(s.projection.inverse().row(3))),
            "g_matProjectionToView" => m(s.projection.inverse()),
            "g_matViewToScreen" | "g_matCurrFrameViewToPrevFrameProj" => m(self.screen_from_clip() * s.projection),
            "g_matPrevProjectionToWorld" => m(forward_z(self.view_proj).inverse()),
            "g_matUnusedMultiview1" => pair(Value::Mat(forward_z(s.camera_relative()))),
            "g_vUnusedMultiview2" => pair(Value::Vec(pos.extend(0.0))),
            "g_vUnusedMultiview3" => pair(Value::Vec((-pos).extend(0.0))),
            "g_vWorldToCameraOffset" => v(-pos.x, -pos.y, -pos.z, 0.0),
            "g_vCameraPositionWs" | "g_vMiddleEyePositionWs" => Some(Value::Vec(pos.extend(0.0))),
            "g_vCameraDirWs" => Some(Value::Vec(s.forward.extend(0.0))),
            "g_vCameraUpDirWs" => Some(Value::Vec(s.up.extend(0.0))),
            "g_flViewportMinZ" => v(self.depth_range[0], 0.0, 0.0, 0.0),
            "g_flViewportMaxZ" => v(self.depth_range[1], 0.0, 0.0, 0.0),
            "g_vDepthPsToVsConversion" => v(s.near * s.far, s.far - s.near, -s.far, 0.0),
            "g_flNearPlane" => v(s.near, 0.0, 0.0, 0.0),
            "g_flFarPlane" => v(s.far, 0.0, 0.0, 0.0),
            "g_flCameraFOV" => v(s.fov_degrees(), 0.0, 0.0, 0.0),
            "g_vCameraAngles" => {
                let a = crate::frustum::camera::angles(s.forward, -s.right, s.up);
                v(a.x.to_radians(), a.y.to_radians(), a.z.to_radians(), 0.0)
            }
            "g_flToneMapScalarLinear" => v(self.tone_map, 0.0, 0.0, 0.0),
            "g_flLightMapScalar" | "g_flEnvMapScalar" | "g_flToneMapScalarGamma" => v(1.0, 0.0, 0.0, 0.0),
            "g_flMod2xIdentity" => v(0.5, 0.0, 0.0, 0.0),
            "g_vInvViewportSize" => v(1.0 / w, 1.0 / h, 0.0, 0.0),
            "g_vViewportOffset" => v(x, y, 0.0, 0.0),
            "g_vViewportSize" => v(w, h, 0.0, 0.0),
            "g_vRenderTargetSize" => v(tw, th, 0.0, 0.0),
            "g_vViewportToGBufferRatio" => v(w / tw, h / th, 0.0, 0.0),
            "g_vMorphTextureAtlasSize" => v(MORPH_ATLAS[0], MORPH_ATLAS[1], 0.0, 0.0),
            "g_vInvGBufferSize" => v(1.0 / tw, 1.0 / th, 1.0, 1.0),
            "g_vFrameBufferCopyInvSizeAndUvScale" => v(1.0 / w, 1.0 / h, 1.0, 1.0),
            "g_nMSAASampleCount" => v(1.0, 0.0, 0.0, 0.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn camera_position_from_view_projection() {
        let eye = Vec3::new(3.0, -2.0, 5.0);
        let vp = glam::camera::rh::proj::directx::perspective(1.0, 1.5, 100.0, 0.5) * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        let view = View { view_proj: vp, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0], ..Default::default() };
        assert!(view.camera_position().truncate().distance(eye) < 1e-3);
        let (forward, up) = view.camera_axes();
        assert!(forward.truncate().distance(-eye.normalize()) < 1e-3, "{forward}");
        assert!(up.truncate().dot(forward.truncate()).abs() < 1e-4 && up.y > 0.5, "{up}");
        let infinite = View { view_proj: render::Camera::perspective(1.0, 1.0, 1.0, eye, Vec3::ZERO).view_proj, ..view };
        assert!(infinite.camera_axes().0.truncate().distance(-eye.normalize()) < 1e-3, "infinite far plane");
    }

    #[test]
    fn world_to_projection_is_camera_relative() {
        let eye = Vec3::new(-260.0, 0.0, 130.0);
        let vp = glam::camera::rh::proj::directx::perspective(1.0, 1.5, 10_000.0, 10.0) * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Z);
        let view = View { view_proj: vp, ..Default::default() };
        let constant = view.constants();
        let (Some(Value::Mat(m)), Some(Value::Vec(offset))) = (constant("g_matWorldToProjection"), constant("g_vWorldToCameraOffset")) else { panic!() };
        let p = Vec3::new(10.0, 20.0, 30.0);
        assert!((m * (p + offset.truncate()).extend(1.0)).abs_diff_eq(vp * p.extend(1.0), 1e-3), "Position3WsToPs");
    }
}
