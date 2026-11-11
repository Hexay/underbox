//! `PerViewConstantBuffer_t` (core `common.fxc`): the per-view constants a translated program
//! reads, from the draw's camera transform and target.

use super::layout::Value;
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
}

/// The block's binding name, as DXC names the cbuffer.
pub const BLOCK: &str = "PerViewConstantBuffer_t";

impl View {
    pub fn with_view_proj(self, view_proj: Mat4) -> View {
        View { view_proj, ..self }
    }

    /// The camera's world position: the point the projection sends to infinity (w = 0). An
    /// orthographic camera has none; it gets the near plane's centre.
    pub fn camera_position(&self) -> Vec4 {
        let inv = self.view_proj.inverse();
        let p = inv * Vec4::new(0.0, 0.0, 1.0, 0.0);
        let p = if p.w.abs() > 1e-6 { p / p.w } else { inv * Vec4::new(0.0, 0.0, 0.0, 1.0) };
        p.truncate().extend(1.0)
    }

    /// The camera's forward and up directions in world space: where clip-space +z and +y lead.
    pub fn camera_axes(&self) -> (Vec4, Vec4) {
        let inv = self.view_proj.inverse();
        let at = |x: f32, y: f32, z: f32| inv.project_point3(glam::Vec3::new(x, y, z));
        // An infinite projection's far plane is at infinity: aim a perspective view from its eye.
        let perspective = self.view_proj.row(3).truncate() != glam::Vec3::ZERO;
        let from = if perspective { self.camera_position().truncate() } else { at(0.0, 0.0, 0.0) };
        let forward = (at(0.0, 0.0, if perspective { 0.5 } else { 1.0 }) - from).normalize_or_zero();
        let up = at(0.0, 1.0, 0.5) - at(0.0, 0.0, 0.5);
        let up = (up - forward * up.dot(forward)).normalize_or_zero();
        (forward.extend(0.0), up.extend(0.0))
    }

    /// Only `[3].w` is real: Fog.hlsl's test for an orthographic view (0 for a perspective one).
    fn view_to_projection(&self) -> Mat4 {
        let perspective = self.view_proj.row(3).truncate() != glam::Vec3::ZERO;
        let mut m = Mat4::IDENTITY;
        m.w_axis.w = if perspective { 0.0 } else { 1.0 };
        m
    }

    /// A global's `Source( ... )` value. `Viewport` is the raster viewport in pixels, which the UI
    /// shaders map screen positions through.
    // TODO: TextureDim / InvTextureDim need the texture the variable is paired with.
    pub fn source(&self, source: &str) -> Option<Vec4> {
        (source == "Viewport").then(|| Vec4::from_array(self.viewport))
    }

    pub fn constant(&self, name: &str) -> Option<Value> {
        let [x, y, w, h] = self.viewport;
        let [tw, th] = self.target;
        let v = |x: f32, y: f32, z: f32, w: f32| Some(Value::Vec(Vec4::new(x, y, z, w)));
        match name {
            "g_matWorldToProjection" => Some(Value::Mat(self.view_proj)),
            "g_matProjectionToWorld" => Some(Value::Mat(self.view_proj.inverse())),
            "g_matViewToProjection" => Some(Value::Mat(self.view_to_projection())),
            "g_matWorldToView" | "g_matProjectionToView" | "g_matViewToScreen" => Some(Value::Mat(Mat4::IDENTITY)),
            "g_vCameraPositionWs" | "g_vMiddleEyePositionWs" => Some(Value::Vec(self.camera_position())),
            "g_vCameraDirWs" => Some(Value::Vec(self.camera_axes().0)),
            "g_vCameraUpDirWs" => Some(Value::Vec(self.camera_axes().1)),
            "g_flToneMapScalarLinear" => v(self.tone_map, 0.0, 0.0, 0.0),
            "g_flLightMapScalar" | "g_flEnvMapScalar" | "g_flToneMapScalarGamma" | "g_flViewportMaxZ" => v(1.0, 0.0, 0.0, 0.0),
            "g_vInvViewportSize" => v(1.0 / w.max(1.0), 1.0 / h.max(1.0), 0.0, 0.0),
            "g_vViewportOffset" => v(x, y, 0.0, 0.0),
            "g_vViewportSize" => v(w, h, 0.0, 0.0),
            "g_vRenderTargetSize" => v(tw, th, 0.0, 0.0),
            "g_vViewportToGBufferRatio" => v(w / tw.max(1.0), h / th.max(1.0), 0.0, 0.0),
            "g_vInvGBufferSize" => v(1.0 / tw.max(1.0), 1.0 / th.max(1.0), 0.0, 0.0),
            "g_nMSAASampleCount" => v(1.0, 0.0, 0.0, 0.0),
            // TODO: time, fog, near/far and the split view/projection need the camera, not view_proj.
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
        let vp = glam::camera::rh::proj::directx::perspective(1.0, 1.5, 0.5, 100.0) * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        let view = View { view_proj: vp, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0], tone_map: 1.0 };
        assert!(view.camera_position().truncate().distance(eye) < 1e-3);
        let (forward, up) = view.camera_axes();
        assert!(forward.truncate().distance(-eye.normalize()) < 1e-3, "{forward}");
        assert!(up.truncate().dot(forward.truncate()).abs() < 1e-4 && up.y > 0.5, "{up}");
        let infinite = View { view_proj: render::Camera::perspective(1.0, 1.0, 1.0, eye, Vec3::ZERO).view_proj, ..view };
        assert!(infinite.camera_axes().0.truncate().distance(-eye.normalize()) < 1e-3, "infinite far plane");
    }
}
