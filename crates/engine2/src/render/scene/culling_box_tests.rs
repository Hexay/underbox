use super::{add, remove, visible};
use crate::math::vec3;
use crate::render::scene::{camera, tests};
use crate::render::{device, texture};
use abi::structs::Angles;
use glam::Vec3;

fn yaw(degrees: f32) -> Angles {
    Angles { yaw: degrees, ..Angles::default() }
}

fn unit_at(x: f32, y: f32) -> (Vec3, Vec3) {
    (Vec3::new(x - 1.0, y - 1.0, -1.0), Vec3::new(x + 1.0, y + 1.0, 1.0))
}

#[test]
fn inside_boxes_hide_what_they_wholly_contain() {
    // Fake world keys: real worlds are heap addresses.
    let w = 11;
    add(w, 1, true, vec3(0.0, 0.0, 0.0), yaw(45.0), vec3(20.0, 20.0, 20.0));
    assert!(!visible(w, unit_at(0.0, 0.0)), "wholly inside");
    assert!(!visible(w, unit_at(9.0, 0.0)), "towards its corner (x 14.14), still wholly inside");
    assert!(visible(w, unit_at(13.5, 0.0)), "partly past its faces");
    assert!(visible(w, unit_at(30.0, 0.0)), "outside");
    remove(w, 1);
    assert!(visible(w, unit_at(0.0, 0.0)), "removed");
}

#[test]
fn outside_boxes_hide_what_touches_none_of_them() {
    let w = 12;
    add(w, 1, false, vec3(0.0, 0.0, 0.0), yaw(0.0), vec3(10.0, 10.0, 10.0));
    add(w, 2, false, vec3(100.0, 0.0, 0.0), yaw(45.0), vec3(10.0, 10.0, 10.0));
    assert!(visible(w, unit_at(0.0, 0.0)), "inside the first");
    assert!(visible(w, unit_at(5.5, 0.0)), "straddling the first's face");
    assert!(!visible(w, unit_at(50.0, 0.0)), "between them");
    assert!(visible(w, unit_at(106.5, 0.0)), "the second's rotated corner reaches x 107.07");
    assert!(!visible(w, unit_at(108.5, 0.0)), "past the second's corner");
    remove(w, 1);
    remove(w, 2);
    assert!(visible(w, unit_at(50.0, 0.0)), "no outside boxes left");
}

/// The scene test's cube at the origin, seen from -64 X: the middle pixel is the cube or the sky.
#[test]
fn a_world_s_culling_boxes_hide_its_objects_from_cameras() {
    device::create().expect("a wgpu adapter");
    let (w, cam) = tests::scene();
    let middle = || {
        let color = texture::create_target("culling box", tests::SIZE, tests::SIZE, 0);
        assert!(camera::render_to_texture(cam, color, Default::default(), Default::default()));
        let px = tests::read(color).texel(32, 32).to_vec();
        crate::resource::destroy_handle(color);
        px
    };
    let sky = vec![115, 153, 217, 255];
    let cube = middle();
    assert_ne!(cube, sky);
    let w = w.addr();
    add(w, 1, true, vec3(0.0, 0.0, 0.0), yaw(0.0), vec3(32.0, 32.0, 32.0));
    assert_eq!(middle(), sky, "an inside box around the cube hides it");
    remove(w, 1);
    add(w, 2, false, vec3(0.0, 300.0, 0.0), yaw(0.0), vec3(32.0, 32.0, 32.0));
    assert_eq!(middle(), sky, "an outside box elsewhere hides it");
    add(w, 3, false, vec3(0.0, 0.0, 0.0), yaw(0.0), vec3(4.0, 4.0, 4.0));
    assert_eq!(middle(), cube, "an outside box it touches keeps it");
    remove(w, 2);
    remove(w, 3);
}
