//! Resting depth against the probe's private world (tools/diff/probe ProbeRest.Depth), headless.

use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;

const DT: f32 = 0.02;
const SUBSTEPS: u32 = 2;

fn slab_world() -> World {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -850.0));
    let ground = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(0.0, 0.0, -8.0)));
    let slab = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(5000.0, 500.0, 8.0) };
    world.add_shape(ground, &slab, Surface::default()).unwrap();
    world
}

fn put(world: &mut World, x: f32, z: f32, shape: Shape) -> RigidBodyHandle {
    let body = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(x, 0.0, z)));
    world.add_shape(body, &shape, Surface::default()).unwrap();
    body
}

fn cube(half: f32) -> Shape {
    Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(half) }
}

/// (name, body, resting centre z) for the probe's `DEPTH` cases.
fn depth_cases(world: &mut World) -> Vec<(String, RigidBodyHandle, f32)> {
    let mut cases = Vec::new();
    for (i, e) in [2.0, 16.0, 64.0].into_iter().enumerate() {
        cases.push((format!("box e={e}"), put(world, -4000.0 + 400.0 * i as f32, e, cube(e)), e));
    }
    let heavy = put(world, -2800.0, 16.0, cube(16.0));
    world.set_mass(heavy, 1000.0);
    cases.push(("box e=16 mass=1000".into(), heavy, 16.0));
    cases.push(("sphere r=16".into(), put(world, -2400.0, 16.0, Shape::Sphere { center: Vector::ZERO, radius: 16.0 }), 16.0));
    for i in 0..3 {
        let z = 16.0 + 32.0 * i as f32;
        cases.push((format!("stack{i} e=16"), put(world, -2000.0, z, cube(16.0)), z));
    }
    cases.push(("box e=16 drop".into(), put(world, -1600.0, 66.0, cube(16.0)), 16.0));
    cases
}

/// Real's sinks after 3 s (`DEPTH t=150`): 0.0086 for any lone body whatever its size or mass,
/// and 0.0257, 0.1399, 0.2057 up a three-box stack. All asleep by then.
#[test]
fn bodies_sink_as_deep_as_real() {
    let mut world = slab_world();
    let cases = depth_cases(&mut world);
    for _ in 0..150 {
        world.step(DT, SUBSTEPS);
    }
    for (name, b, rest) in cases {
        let sink = rest - world.body(b).unwrap().translation().z;
        let (want, tol) = match name.as_str() {
            "stack0 e=16" => (0.0257, 0.002),
            "stack1 e=16" => (0.1399, 0.01),
            "stack2 e=16" => (0.2057, 0.01),
            _ => (0.0086, 0.0005),
        };
        assert!((sink - want).abs() < tol, "{name}: sink {sink}, real {want}");
        assert!(world.body(b).unwrap().is_sleeping(), "{name} asleep by 3 s");
    }
}
