use super::{Joint, JointKind, Motor};
use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;
use std::f32::consts::FRAC_PI_2;

fn body(world: &mut World, kind: BodyType, at: Vector) -> RigidBodyHandle {
    let b = world.add_body(kind, Pose::from_translation(at));
    let cube = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(4.0) };
    world.add_shape(b, &cube, Surface::default()).unwrap();
    b
}

fn run(world: &mut World, steps: usize) {
    for _ in 0..steps {
        world.step(1.0 / 60.0, 2);
    }
}

/// A static anchor at the origin and a dynamic arm 20in out along +X.
fn arm(world: &mut World) -> (RigidBodyHandle, RigidBodyHandle) {
    (body(world, BodyType::Static, Vector::ZERO), body(world, BodyType::Dynamic, Vector::new(20.0, 0.0, 0.0)))
}

/// Frames at the anchor's origin whose Z is world Y, so gravity (−Z) swings the arm about it.
fn hinge(kind: JointKind) -> Joint {
    let axis = Rotation::from_rotation_x(-FRAC_PI_2);
    Joint::new(kind, Pose::from_parts(Vector::ZERO, axis), Pose::from_parts(Vector::new(-20.0, 0.0, 0.0), axis))
}

#[test]
fn hinge_swings_freely_and_respects_limits() {
    let mut world = World::default();
    let (a, b) = arm(&mut world);
    let free = world.add_joint(a, b, hinge(JointKind::Revolute)).unwrap();
    run(&mut world, 120);
    let p = world.body(b).unwrap().translation();
    assert!(p.length() > 19.0 && p.length() < 21.0, "arm stays on its radius: {p:?}");
    assert!(world.joint_angle(free).abs() > 0.5, "free hinge swings down: {}", world.joint_angle(free));

    let mut world = World::default();
    let (a, b) = arm(&mut world);
    let limited = world.add_joint(a, b, hinge(JointKind::Revolute)).unwrap();
    let limit = 20f32.to_radians();
    world.update_joint(limited, |j| {
        j.twist_limits = [-limit, limit];
        j.twist_limit_enabled = true;
    });
    run(&mut world, 120);
    let angle = world.joint_angle(limited);
    assert!(angle.abs() <= limit + 0.05, "held at the limit: {angle}");
    assert!(angle.abs() > limit - 0.1, "rests against it: {angle}");
}

#[test]
fn hinge_motor_drives_its_speed() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let (a, b) = arm(&mut world);
    let id = world.add_joint(a, b, hinge(JointKind::Revolute)).unwrap();
    world.update_joint(id, |j| j.angular_motor = Some(Motor { speed: 2.0, max: 1.0e7 }));
    run(&mut world, 30);
    let spin = world.body(b).unwrap().angvel().length();
    assert!((spin - 2.0).abs() < 0.1, "spins at the motor's speed: {spin}");
}

#[test]
fn weld_holds_and_breaks_past_its_strength() {
    let mut world = World::default();
    let (a, b) = arm(&mut world);
    let weld = Joint::new(JointKind::Weld, Pose::IDENTITY, Pose::from_translation(Vector::new(-20.0, 0.0, 0.0)));
    let id = world.add_joint(a, b, weld).unwrap();
    world.set_joint_user_data(id, 7);
    run(&mut world, 60);
    assert!((world.body(b).unwrap().translation() - Vector::new(20.0, 0.0, 0.0)).length() < 0.5, "weld holds the arm up");
    let (linear, _) = world.joint_impulses(id);
    assert!(linear > 0.0, "it measures the load");

    world.update_joint(id, |j| j.max_linear_impulse = linear * 0.5);
    run(&mut world, 1);
    assert_eq!(world.take_broken_joints(), [(id, 7)]);
    assert!(world.joint(id).is_none());
    run(&mut world, 30);
    assert!(world.body(b).unwrap().translation().z < -5.0, "falls once broken");
}

#[test]
fn ball_socket_keeps_the_anchor_and_swing_limit() {
    let mut world = World::default();
    let (a, b) = arm(&mut world);
    // Twist about world Z, so the arm swinging down about Y is swing.
    let socket = Joint::new(JointKind::Spherical, Pose::IDENTITY, Pose::from_translation(Vector::new(-20.0, 0.0, 0.0)));
    let id = world.add_joint(a, b, socket).unwrap();
    world.update_joint(id, |j| {
        j.swing_limit = 0.3;
        j.swing_limit_enabled = true;
    });
    run(&mut world, 120);
    let anchor = *world.body(b).unwrap().position() * Vector::new(-20.0, 0.0, 0.0);
    assert!(anchor.length() < 0.5, "anchors stay together: {anchor:?}");
    let drop = -world.body(b).unwrap().translation().z;
    assert!(drop < 20.0 * 0.35, "swing limited to ~0.3 rad: dropped {drop}");
}

#[test]
fn slider_moves_only_along_its_axis_within_limits() {
    let mut world = World::default();
    let (a, b) = arm(&mut world);
    // Frame Z along world −Z: the arm slides down under gravity.
    let down = Rotation::from_rotation_x(std::f32::consts::PI);
    let frames = Joint::new(JointKind::Prismatic, Pose::from_parts(Vector::ZERO, down), Pose::from_parts(Vector::new(-20.0, 0.0, 0.0), down));
    let id = world.add_joint(a, b, frames).unwrap();
    world.update_joint(id, |j| {
        j.linear_limits = [0.0, 15.0];
        j.linear_limit_enabled = true;
    });
    run(&mut world, 120);
    let p = world.body(b).unwrap().translation();
    assert!((p.z + 15.0).abs() < 0.5, "stops at the limit: {p:?}");
    assert!((p.x - 20.0).abs() < 0.1 && p.y.abs() < 0.1, "no sideways motion: {p:?}");
}

#[test]
fn spring_settles_near_rest_length_and_filter_disables_contact() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let (a, b) = arm(&mut world);
    let id = world.add_joint(a, b, Joint::new(JointKind::Spring, Pose::IDENTITY, Pose::IDENTITY)).unwrap();
    world.update_joint(id, |j| j.linear_spring = [2.0, 1.0, 40.0]);
    run(&mut world, 240);
    let d = world.body(b).unwrap().translation().length();
    assert!((d - 40.0).abs() < 1.0, "spring pushes out to rest: {d}");

    let mut world = World::default();
    let ground = world.add_body(BodyType::Static, Pose::IDENTITY);
    world.add_shape(ground, &Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(50.0, 50.0, 2.0) }, Surface::default());
    let ball = body(&mut world, BodyType::Dynamic, Vector::new(0.0, 0.0, 10.0));
    world.add_joint(ground, ball, Joint::new(JointKind::Filter, Pose::IDENTITY, Pose::IDENTITY)).unwrap();
    run(&mut world, 60);
    assert!(world.body(ball).unwrap().translation().z < -10.0, "filtered pair passes through");
}

#[test]
fn removing_a_body_drops_its_joints() {
    let mut world = World::default();
    let (a, b) = arm(&mut world);
    let id = world.add_joint(a, b, hinge(JointKind::Revolute)).unwrap();
    world.remove_body(b);
    assert!(world.joint(id).is_none());
}
