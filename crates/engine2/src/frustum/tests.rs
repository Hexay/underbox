use super::camera::{Camera, angles, basis};
use glam::{Mat4, Vec3, Vec4};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0)
}

fn close3(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

fn camera() -> Camera {
    let mut c = Camera { origin: Vec3::new(10.0, 20.0, 30.0), near: 5.0, far: 5000.0, fov: 70.0, aspect: 16.0 / 9.0, ..Camera::default() };
    c.set_angles(Vec3::new(15.0, 40.0, 0.0));
    c
}

/// Sandbox.SceneRenderer's RenderView: translation * WorldToViewRotation * ReverseZPerspective,
/// row-vector, which is what managed reads from GetReverseZViewProjTranspose.
fn managed_reverse_clip(c: &Camera, p: Vec3) -> Vec4 {
    let rel = p - c.origin;
    let view = Vec3::new(rel.dot(c.right()), rel.dot(c.up), -rel.dot(c.forward));
    let x = 1.0 / c.tan_half_fov();
    let range = c.far - c.near;
    Vec4::new(view.x * x, view.y * x * c.aspect, view.z * c.near / range + c.far * c.near / range, -view.z)
}

#[test]
fn reverse_z_view_proj_matches_managed_renderer() {
    let c = camera();
    let m = Camera::reverse_z(c.proj()) * c.view();
    for p in [Vec3::new(500.0, 300.0, 100.0), Vec3::new(-40.0, 900.0, -60.0), Vec3::new(12.0, 21.0, 30.0)] {
        let ours = m * p.extend(1.0);
        let theirs = managed_reverse_clip(&c, p);
        assert!(ours.abs_diff_eq(theirs, 1e-2 * theirs.abs().max_element().max(1.0)), "{ours} vs {theirs}");
    }
    let near = c.origin + c.forward * c.near;
    let far = c.origin + c.forward * c.far;
    let depth = |p: Vec3| {
        let v = m * p.extend(1.0);
        v.z / v.w
    };
    assert!(close(depth(near), 1.0) && close(depth(far), 0.0), "reverse Z: near 1, far 0");
    let fwd = c.view_proj() * near.extend(1.0);
    assert!(close(fwd.z / fwd.w, 0.0), "forward Z: near 0");
}

#[test]
fn ortho_matches_managed_reverse_z_orthographic() {
    let mut c = camera();
    c.ortho = Some((800.0, 450.0));
    let p = Camera::reverse_z(c.proj());
    let range = c.far - c.near;
    let expect = Mat4::from_cols_array(&[2.0 / 800.0, 0.0, 0.0, 0.0, 0.0, 2.0 / 450.0, 0.0, 0.0, 0.0, 0.0, 1.0 / range, 0.0, 0.0, 0.0, c.far / range, 1.0]);
    assert!(p.abs_diff_eq(expect, 1e-6), "{p} vs {expect}");
}

#[test]
fn planes_are_inward_and_camera_relative() {
    let c = camera();
    let planes = c.planes();
    assert!(close3(planes[4].0, c.forward) && close(planes[4].1, c.near));
    assert!(close3(planes[5].0, -c.forward) && close(planes[5].1, -c.far));
    for (n, d) in &planes[..4] {
        assert!(close(*d, 0.0), "side planes pass through the camera");
        assert!(n.dot(c.forward) > 0.0, "inward");
    }
    assert!(planes[0].0.dot(c.right()) < 0.0 && planes[1].0.dot(c.right()) > 0.0, "right then left");
    assert!(planes[2].0.dot(c.up) < 0.0 && planes[3].0.dot(c.up) > 0.0, "top then bottom");

    let ahead = c.origin + c.forward * 100.0;
    assert!(c.intersects_box(ahead - 1.0, ahead + 1.0));
    let behind = c.origin - c.forward * 100.0;
    assert!(!c.intersects_box(behind - 1.0, behind + 1.0));
    assert!(c.intersects_box(c.origin - 1000.0, c.origin + 1000.0), "a box around the camera");
}

#[test]
fn screen_transform_and_unproject() {
    let c = camera();
    let (s, behind) = c.screen_transform(c.origin + c.forward * 50.0);
    assert!(!behind && s.x.abs() < 1e-4 && s.y.abs() < 1e-4);
    assert!(c.screen_transform(c.origin - c.forward * 50.0).1, "behind");
    let (s, _) = c.screen_transform(c.origin + c.forward * 50.0 + c.right() * 5.0 + c.up * 5.0);
    assert!(s.x > 0.0 && s.y > 0.0, "+x right, +y up");

    assert!(close3(c.unproject_near(0.0, 0.0), c.origin + c.forward * c.near));
    let p = c.unproject_near(0.3, -0.6);
    let (s, _) = c.screen_transform(p);
    assert!(close(s.x, 0.3) && close(s.y, -0.6));
    let edge = c.unproject_near(1.0, 0.0) - c.origin;
    assert!(close(edge.dot(c.right()) / c.near, c.tan_half_fov()), "horizontal field of view");
}

#[test]
fn clip_window_zooms_into_a_sub_rect() {
    let full = camera();
    let mut part = camera();
    part.window = [-1.0, -1.0, 0.0, 0.0];
    let p = full.unproject_near(-0.5, -0.5);
    let (s, _) = part.screen_transform(p);
    assert!(close(s.x, 0.0) && close(s.y, 0.0), "{s}");
}

#[test]
fn screen_size_matches_light_binner() {
    let c = camera();
    let center = c.origin + c.forward * 400.0;
    assert!(close(c.screen_size(center, 10.0), 10.0 / (400.0 * c.tan_half_fov())));
    assert_eq!(c.screen_size(c.origin, 10.0), 1.0, "inside the sphere");
}

#[test]
fn angles_and_basis_round_trip() {
    for a in [Vec3::new(15.0, 40.0, 0.0), Vec3::new(-30.0, -120.0, 25.0), Vec3::ZERO] {
        let (f, l, u) = basis(a);
        assert!(close3(angles(f, l, u), a), "{a}");
    }
    let (f, l, u) = basis(Vec3::ZERO);
    assert!(close3(f, Vec3::X) && close3(l, Vec3::Y) && close3(u, Vec3::Z), "Z-up, x forward, y left");
    let (f, _, _) = basis(Vec3::new(90.0, 0.0, 0.0));
    assert!(close3(f, -Vec3::Z), "positive pitch looks down");
}
