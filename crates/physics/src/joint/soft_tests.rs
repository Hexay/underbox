//! Joints against contacts they share, as the probe's private worlds measure them on real engine2
//! (tools/diff/probe ProbeJoint: `OVERLAP`, `CAR`), headless.

use super::{Joint, JointKind, Motor};
use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;
use std::f32::consts::FRAC_PI_2;

fn put(world: &mut World, at: Vector, shape: Shape) -> RigidBodyHandle {
    let body = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
    world.add_shape(body, &shape, Surface::default()).unwrap();
    body
}

fn cuboid(half: Vector) -> Shape {
    Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: half }
}

fn hinge(frame1: Pose, frame2: Pose, max: f32) -> Joint {
    let mut j = Joint::new(JointKind::Revolute, frame1, frame2);
    j.collide = true;
    j.angular_motor = Some(Motor { speed: 0.0, max });
    j
}

/// Two 16-unit boxes overlapping by 4, hinged where they meet, gravity off: real pushes each 0.4
/// out against the joint and holds there.
#[test]
fn joint_yields_to_an_overlap_it_collides_with() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let a = put(&mut world, Vector::ZERO, cuboid(Vector::splat(8.0)));
    let b = put(&mut world, Vector::new(12.0, 0.0, 0.0), cuboid(Vector::splat(8.0)));
    let frame = |x| Pose::from_translation(Vector::new(x, 0.0, 0.0));
    world.add_joint(a, b, hinge(frame(6.0), frame(-6.0), 9.2)).unwrap();
    for _ in 0..50 {
        world.step(0.02, 2);
    }
    let gap = world.body(b).unwrap().translation().x - world.body(a).unwrap().translation().x;
    assert!((gap - 12.8).abs() < 0.1, "real ends 12.8 apart: {gap}");
}

/// A hinge spring to 1 rad from rest, gravity off: real's spin after each step (probe `HINGE spring`).
#[test]
fn hinge_spring_follows_real() {
    for (hertz, zeta, real) in [(1.0, 0.0, [0.7837, 1.5490, 2.2839]), (1.0, 1.0, [0.6749, 1.1908, 1.5760]), (5.0, 0.3, [13.4333, 17.4711, 14.8843])] {
        let mut world = World::default();
        world.set_gravity(Vector::ZERO);
        let anchor = world.add_body(BodyType::Static, Pose::IDENTITY);
        let wheel = put(&mut world, Vector::ZERO, Shape::Sphere { center: Vector::ZERO, radius: 8.0 });
        let mut j = Joint::new(JointKind::Revolute, Pose::IDENTITY, Pose::IDENTITY);
        j.angular_spring = [1.0, hertz, zeta];
        world.add_joint(anchor, wheel, j).unwrap();
        for want in real {
            world.step(0.02, 2);
            let w = world.body(wheel).unwrap().angvel().z;
            assert!((w - want).abs() < 0.02 * want + 0.005, "{hertz} Hz ζ {zeta}: {w}, real {want}");
        }
    }
}

/// A box chassis on a slab with three sphere wheels hinged inside it: real lifts it 0.75.
#[test]
fn wheels_inside_their_chassis_lift_it() {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -850.0));
    let ground = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(0.0, 0.0, -8.0)));
    world.add_shape(ground, &cuboid(Vector::new(500.0, 500.0, 8.0)), Surface::default()).unwrap();
    let chassis = put(&mut world, Vector::new(0.0, 0.0, 8.0), cuboid(Vector::new(12.0, 8.0, 4.0)));
    world.set_mass(chassis, 6.0);
    let axle = Rotation::from_rotation_x(FRAC_PI_2);
    for (i, at) in [Vector::new(8.0, 6.0, -4.0), Vector::new(8.0, -6.0, -4.0), Vector::new(-8.0, 0.0, -4.0)].into_iter().enumerate() {
        let wheel = put(&mut world, Vector::new(0.0, 0.0, 8.0) + at, Shape::Sphere { center: Vector::ZERO, radius: 4.0 });
        world.set_mass(wheel, 1.0);
        let max = if i == 2 { 506.5 } else { 9.23 };
        world.add_joint(chassis, wheel, hinge(Pose::from_parts(at, axle), Pose::from_parts(Vector::ZERO, axle), max)).unwrap();
    }
    for _ in 0..100 {
        world.step(0.02, 2);
    }
    let rb = world.body(chassis).unwrap();
    assert!((rb.translation().z - 8.7465).abs() < 0.1, "real rests at 8.7465: {}", rb.translation().z);
    assert!(rb.is_sleeping(), "real is asleep by step 50; a rigid-contact fight keeps creeping");
}
