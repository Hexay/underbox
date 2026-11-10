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

    pub fn constant(&self, name: &str) -> Option<Value> {
        let [x, y, w, h] = self.viewport;
        let [tw, th] = self.target;
        let v = |x: f32, y: f32, z: f32, w: f32| Some(Value::Vec(Vec4::new(x, y, z, w)));
        match name {
            "g_matWorldToProjection" => Some(Value::Mat(self.view_proj)),
            "g_matProjectionToWorld" => Some(Value::Mat(self.view_proj.inverse())),
            "g_matWorldToView" | "g_matViewToProjection" | "g_matProjectionToView" | "g_matViewToScreen" => Some(Value::Mat(Mat4::IDENTITY)),
            "g_vCameraPositionWs" | "g_vMiddleEyePositionWs" => Some(Value::Vec(self.camera_position())),
            "g_flToneMapScalarLinear" | "g_flLightMapScalar" | "g_flEnvMapScalar" | "g_flToneMapScalarGamma" | "g_flViewportMaxZ" => v(1.0, 0.0, 0.0, 0.0),
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
        let view = View { view_proj: vp, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0] };
        assert!(view.camera_position().truncate().distance(eye) < 1e-3);
    }
}
