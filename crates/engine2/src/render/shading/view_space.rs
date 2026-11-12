//! A reverse-Z world → clip matrix taken apart into what native keeps separately: the eye, the
//! camera's axes and its projection. A perspective sends the eye to clip w = 0 and its row 3 is
//! the forward axis; an orthographic view's eye is the centre of its near plane (clip z = 1).

use glam::{Mat3, Mat4, Vec3, Vec4};

#[derive(Clone, Copy, Debug)]
pub struct Space {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// World directions → view (right, up, back), as native's `WorldToView` rotates.
    pub rotation: Mat4,
    /// View → clip: `view_proj = projection · rotation · translate(-position)`.
    pub projection: Mat4,
    pub near: f32,
    pub far: f32,
    pub perspective: bool,
}

fn unit(v: Vec3, fallback: Vec3) -> Vec3 {
    v.try_normalize().unwrap_or(fallback)
}

impl Space {
    pub fn of(view_proj: Mat4) -> Space {
        let perspective = view_proj.row(3).truncate() != Vec3::ZERO;
        let inv = view_proj.inverse();
        let eye = inv * Vec4::new(0.0, 0.0, 1.0, 0.0);
        let position = if perspective && eye.w.abs() > 1e-6 { eye.truncate() / eye.w } else { inv.project_point3(Vec3::Z) };
        let forward = if perspective { unit(view_proj.row(3).truncate(), Vec3::NEG_Z) } else { unit(-view_proj.row(2).truncate(), Vec3::NEG_Z) };
        let up = view_proj.row(1).truncate();
        let up = unit(up - forward * up.dot(forward), forward.any_orthonormal_vector());
        let right = forward.cross(up);
        let rotation = Mat4::from_mat3(Mat3::from_cols(right, up, -forward).transpose());
        let mut projection = view_proj * Mat4::from_translation(position) * rotation.transpose();
        if !perspective {
            // An ortho projection only scales each axis and offsets z (the eye is clip (0, 0, 1));
            // native's holds exact zeros where taking the matrix apart leaves float noise.
            (projection.x_axis.y, projection.x_axis.z, projection.y_axis.x, projection.y_axis.z, projection.z_axis.x, projection.z_axis.y) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            (projection.w_axis.x, projection.w_axis.y) = (0.0, 0.0);
        }
        let (a, b) = (projection.z_axis.z, projection.w_axis.z);
        let (near, far) = if perspective { (b / (1.0 + a), b / a) } else { ((b - 1.0) / a, b / a) };
        let finite = |x: f32, or: f32| if x.is_finite() { x } else { or };
        // A shadow cascade's near plane is 0: b - 1 is then rounding, scaled up by the far plane.
        let near = finite(near, 0.0);
        let near = if !perspective && near.abs() < far.abs() * 1e-6 { 0.0 } else { near };
        Space { position, forward, up, right, rotation, projection, near, far: finite(far, near.max(1.0) * 1e6), perspective }
    }

    /// World → clip about the eye: what `Position3WsToPs` multiplies after adding
    /// `g_vWorldToCameraOffset`.
    pub fn camera_relative(&self) -> Mat4 {
        self.projection * self.rotation
    }

    /// The horizontal field of view in degrees; -1 for an orthographic view (native's convention).
    pub fn fov_degrees(&self) -> f32 {
        if self.perspective { 2.0 * (1.0 / self.projection.x_axis.x.abs()).atan().to_degrees() } else { -1.0 }
    }
}

/// Native's forward-Z twin of a reverse-Z clip matrix: depth `w - z`.
pub fn forward_z(m: Mat4) -> Mat4 {
    crate::frustum::camera::Camera::reverse_z(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::camera::rh;

    #[test]
    fn a_reverse_perspective_comes_apart_into_eye_axes_and_planes() {
        let eye = Vec3::new(-260.0, 0.0, 130.0);
        let target = eye + Vec3::new(0.901_888, 0.0, -0.431_969);
        let proj = rh::proj::directx::perspective(1.0, 4.0 / 3.0, 10_000.0, 10.0);
        let s = Space::of(proj * rh::view::look_at_mat4(eye, target, Vec3::Z));
        assert!(s.position.distance(eye) < 1e-2, "{}", s.position);
        assert!(s.forward.distance(Vec3::new(0.901_888, 0.0, -0.431_969)) < 1e-4, "{}", s.forward);
        assert!(s.right.distance(-Vec3::Y) < 1e-4 && s.up.z > 0.9, "{} {}", s.right, s.up);
        assert!((s.near - 10.0).abs() < 1e-2 && (s.far - 10_000.0).abs() < 1.0, "{} {}", s.near, s.far);
        assert!(s.projection.abs_diff_eq(proj, 1e-4), "{}", s.projection);
    }

    #[test]
    fn a_reverse_ortho_has_its_eye_on_the_near_plane() {
        let eye = Vec3::new(5.0, 6.0, 500.0);
        let proj = rh::proj::directx::orthographic(-100.0, 100.0, -100.0, 100.0, 1000.0, 0.0);
        let s = Space::of(proj * rh::view::look_at_mat4(eye, eye - Vec3::Z, Vec3::Y));
        assert!(!s.perspective && s.position.distance(eye) < 1e-3, "{}", s.position);
        assert!(s.forward.distance(-Vec3::Z) < 1e-5 && s.near.abs() < 1e-3 && (s.far - 1000.0).abs() < 1e-2, "{} {}", s.near, s.far);
        assert_eq!(s.fov_degrees(), -1.0);
    }
}
