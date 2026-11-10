//! `CFrustum` maths in Source 2's conventions: Z-up world (x forward, y left), right-handed view
//! space (+x right, +y up, looking down -z), column-vector matrices, D3D depth (forward Z: near 0,
//! far 1; reverse Z: near 1, far 0), and a horizontal field of view in degrees.

use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub origin: Vec3,
    /// Pitch, yaw, roll in degrees.
    pub angles: Vec3,
    pub forward: Vec3,
    pub left: Vec3,
    pub up: Vec3,
    pub near: f32,
    pub far: f32,
    pub fov: f32,
    pub aspect: f32,
    /// Full width and height of an orthographic view, in world units.
    pub ortho: Option<(f32, f32)>,
    /// The part of clip space (bottom-left x, y, top-right x, y) the view covers.
    pub window: [f32; 4],
    /// Viewport size, from `SetCameraWidthHeight`.
    pub size: (f32, f32),
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Self {
            origin: Vec3::ZERO,
            angles: Vec3::ZERO,
            forward: Vec3::X,
            left: Vec3::Y,
            up: Vec3::Z,
            near: 1.0,
            far: 1000.0,
            fov: 90.0,
            aspect: 1.0,
            ortho: None,
            window: [-1.0, -1.0, 1.0, 1.0],
            size: (0.0, 0.0),
        };
        c.set_angles(Vec3::ZERO);
        c
    }
}

/// Source's `AngleVectors`: forward, left and up for pitch/yaw/roll in degrees.
pub fn basis(angles: Vec3) -> (Vec3, Vec3, Vec3) {
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sy, cy) = angles.y.to_radians().sin_cos();
    let (sr, cr) = angles.z.to_radians().sin_cos();
    let forward = Vec3::new(cp * cy, cp * sy, -sp);
    let right = Vec3::new(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (forward, -right, up)
}

/// Source's `MatrixAngles` on a forward/left/up basis.
pub fn angles(forward: Vec3, left: Vec3, up: Vec3) -> Vec3 {
    let xy = forward.x.hypot(forward.y);
    let pitch = (-forward.z).atan2(xy);
    let (yaw, roll) =
        if xy > 0.001 { (forward.y.atan2(forward.x), left.z.atan2(up.z)) } else { ((-left.x).atan2(left.y), 0.0) };
    Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees())
}

impl Camera {
    pub fn set_angles(&mut self, angles: Vec3) {
        self.angles = angles;
        (self.forward, self.left, self.up) = basis(angles);
    }

    pub fn right(&self) -> Vec3 {
        -self.left
    }

    /// World to view, rotation only (camera-relative positions in).
    pub fn view_rotation(&self) -> Mat4 {
        let (r, u, b) = (self.right(), self.up, -self.forward);
        Mat4::from_cols(
            Vec4::new(r.x, u.x, b.x, 0.0),
            Vec4::new(r.y, u.y, b.y, 0.0),
            Vec4::new(r.z, u.z, b.z, 0.0),
            Vec4::W,
        )
    }

    pub fn view(&self) -> Mat4 {
        self.view_rotation() * Mat4::from_translation(-self.origin)
    }

    pub fn tan_half_fov(&self) -> f32 {
        (self.fov.to_radians() * 0.5).tan()
    }

    /// Forward-Z projection (`MatrixBuildPerspectiveX` / `MatrixBuildOrtho`), then the clip window.
    pub fn proj(&self) -> Mat4 {
        let (n, f) = (self.near, self.far);
        let base = match self.ortho {
            Some((w, h)) => Mat4::from_cols(
                Vec4::new(2.0 / w, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 2.0 / h, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0 / (n - f), 0.0),
                Vec4::new(0.0, 0.0, n / (n - f), 1.0),
            ),
            None => {
                let x = 1.0 / self.tan_half_fov();
                Mat4::from_cols(
                    Vec4::new(x, 0.0, 0.0, 0.0),
                    Vec4::new(0.0, x * self.aspect, 0.0, 0.0),
                    Vec4::new(0.0, 0.0, f / (n - f), -1.0),
                    Vec4::new(0.0, 0.0, n * f / (n - f), 0.0),
                )
            }
        };
        self.window_matrix() * base
    }

    /// Maps the clip window to the full -1..1 range.
    fn window_matrix(&self) -> Mat4 {
        let [x0, y0, x1, y1] = self.window;
        let (sx, sy) = (2.0 / (x1 - x0), 2.0 / (y1 - y0));
        let mut m = Mat4::from_scale(Vec3::new(sx, sy, 1.0));
        m.w_axis = Vec4::new(-(x0 + x1) * 0.5 * sx, -(y0 + y1) * 0.5 * sy, 0.0, 1.0);
        m
    }

    /// Depth `w - z`: forward Z's near 0 / far 1 become 1 / 0.
    pub fn reverse_z(proj: Mat4) -> Mat4 {
        let flip = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, -1.0, 0.0), Vec4::new(0.0, 0.0, 1.0, 1.0));
        flip * proj
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj() * self.view()
    }

    /// Inward planes `(normal, distance)` with `normal . (p - origin) >= distance` inside, in
    /// `CFrustum` order: right, left, top, bottom, near, far.
    pub fn planes(&self) -> [(Vec3, f32); 6] {
        let m = self.proj() * self.view_rotation();
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        [r3 - r0, r3 + r0, r3 - r1, r3 + r1, r2, r3 - r2].map(|p| {
            let len = p.truncate().length();
            if len > 0.0 { (p.truncate() / len, -p.w / len) } else { (Vec3::ZERO, 0.0) }
        })
    }

    /// False only when the box is wholly outside one plane.
    pub fn intersects_box(&self, mins: Vec3, maxs: Vec3) -> bool {
        self.planes().iter().all(|&(n, d)| {
            let corner = Vec3::select(n.cmpge(Vec3::ZERO), maxs, mins);
            n.dot(corner - self.origin) >= d
        })
    }

    /// Clip-space x, y over w, and whether the point is behind the camera (`ScreenTransform`).
    pub fn screen_transform(&self, world: Vec3) -> (Vec3, bool) {
        let clip = self.view_proj() * world.extend(1.0);
        if clip.w < 0.001 {
            return (Vec3::new(clip.x * 100_000.0, clip.y * 100_000.0, 0.0), true);
        }
        (Vec3::new(clip.x / clip.w, clip.y / clip.w, 0.0), false)
    }

    /// The world point on the near plane at clip-space `x`, `y`.
    pub fn unproject_near(&self, x: f32, y: f32) -> Vec3 {
        let p = self.view_proj().inverse() * Vec4::new(x, y, 0.0, 1.0);
        if p.w.abs() > f32::EPSILON { p.truncate() / p.w } else { self.origin }
    }

    /// The fraction of the view's half-width a sphere covers, 1 when the camera is inside it.
    pub fn screen_size(&self, center: Vec3, radius: f32) -> f32 {
        let half_width = match self.ortho {
            Some((w, _)) => w * 0.5,
            None => {
                let distance = center.distance(self.origin);
                if distance < radius {
                    return 1.0;
                }
                distance * self.tan_half_fov()
            }
        };
        if half_width > 0.0 { (radius / half_width).clamp(0.0, 1.0) } else { 1.0 }
    }
}
