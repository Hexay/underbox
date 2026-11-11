//! The differential probe's drops (tools/diff/probe), headless: a 32-unit box dropped from 200
//! onto a slab whose top is z = 0, at 50 Hz with managed's 2 sub-steps. Real engine2's results
//! are in docs/differential.md (divergence 2).

use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;

const DT: f32 = 0.02;
const SUBSTEPS: u32 = 2;

fn scene() -> World {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -850.0));
    let ground = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(0.0, 0.0, -8.0)));
    let slab = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(2000.0, 2000.0, 8.0) };
    world.add_shape(ground, &slab, Surface::default()).unwrap();
    world
}

/// s&box `Rotation.From(pitch, yaw, roll)`, degrees.
fn angles(pitch: f32, yaw: f32, roll: f32) -> Rotation {
    let r = |d: f32| d.to_radians();
    Rotation::from_rotation_z(r(yaw)) * Rotation::from_rotation_y(r(pitch)) * Rotation::from_rotation_x(r(roll))
}

fn drop_box(world: &mut World, at: Vector, rotation: Rotation) -> RigidBodyHandle {
    let body = world.add_body(BodyType::Dynamic, Pose::from_parts(at, rotation));
    let cube = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(16.0) };
    world.add_shape(body, &cube, Surface::default()).unwrap();
    body
}

/// (distance from the drop point in the plane, speed at tick 50).
fn land(pitch: f32, yaw: f32, roll: f32) -> (f32, f32) {
    let mut world = scene();
    let body = drop_box(&mut world, Vector::new(0.0, 0.0, 200.0), angles(pitch, yaw, roll));
    let mut speed = 0.0;
    for tick in 1..=150 {
        world.step(DT, SUBSTEPS);
        if tick == 50 {
            speed = world.body(body).unwrap().linvel().length();
        }
    }
    let p = world.body(body).unwrap().translation();
    (Vector::new(p.x, p.y, 0.0).length(), speed)
}

#[test]
fn free_fall_integrates_four_times_a_tick() {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -850.0));
    let ball = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
    world.add_shape(ball, &Shape::Sphere { center: Vector::ZERO, radius: 8.0 }, Surface::default()).unwrap();
    for _ in 0..10 {
        world.step(DT, SUBSTEPS);
    }
    let (n, k) = (10.0, 4.0);
    let want = -850.0 * DT * DT * n * k * (n * k + 1.0) / (2.0 * k * k);
    let z = world.body(ball).unwrap().translation().z;
    assert!((z - want).abs() < 1e-2, "z {z}, semi-implicit Euler with 4 integrations a tick gives {want}");
}

/// Real lands this box 5.8 units from the drop point, at 0.5 u/s by tick 50 (Rapier's defaults: 58, 92).
#[test]
fn tilted_box_lands_near_its_drop_point() {
    let (moved, speed) = land(20.0, 30.0, 10.0);
    assert!(moved < 15.0 && speed < 10.0, "moved {moved}, speed at tick 50 {speed}");
}

#[test]
fn eight_box_stack_stands() {
    let mut world = scene();
    let stack: Vec<_> = (0..8).map(|i| drop_box(&mut world, Vector::new(0.0, 0.0, 16.0 + 32.0 * i as f32), Rotation::IDENTITY)).collect();
    for _ in 0..500 {
        world.step(DT, SUBSTEPS);
    }
    let top = world.body(stack[7]).unwrap().translation();
    assert!(top.z > 235.0 && Vector::new(top.x, top.y, 0.0).length() < 2.0, "top box at {top:?}");
}

#[test]
#[ignore = "diagnostic: tools/test.sh physics print_landings -- --ignored --nocapture"]
fn print_landings() {
    for a in [(20.0, 30.0, 10.0), (0.0, 0.0, 0.0), (45.0, 0.0, 0.0), (0.0, 0.0, 45.0), (30.0, 45.0, 15.0), (60.0, 10.0, 0.0), (10.0, 10.0, 10.0), (89.0, 0.0, 0.0), (35.0, 45.0, 0.0)] {
        let (d, v) = land(a.0, a.1, a.2);
        println!("{a:?}: moved {d:.1}, speed@50 {v:.1}");
    }
}
