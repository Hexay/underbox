//! The zoo hoover's trike spawned through its error sign, headless, against real's probe `TRIKE`
//! markers (tools/diff/probe/Code/ProbeTrike.cs; docs/differential-joints.md, the zoo hoover).

use crate::joint::{Joint, JointKind, Motor};
use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;
use std::f32::consts::{FRAC_PI_2, TAU};

fn cuboid(min: [f32; 3], max: [f32; 3]) -> Shape {
    let (min, max) = (Vector::from(min), Vector::from(max));
    Shape::Box { center: (min + max) / 2.0, rotation: Rotation::IDENTITY, half_extents: (max - min) / 2.0 }
}

/// AddCylinderShape's hull: 16 slices about X (the probe's roll leaves that axis), centred on `center`.
fn wheel(center: Vector, height: f32, radius: f32) -> Shape {
    let points = (0..16)
        .flat_map(|i| {
            let a = TAU * i as f32 / 16.0;
            let (y, z) = (radius * a.cos(), radius * a.sin());
            [Vector::new(-height / 2.0, y, z), Vector::new(height / 2.0, y, z)]
        })
        .collect();
    Shape::Hull { pose: Pose::from_translation(center), points }
}

fn body(world: &mut World, at: Vector, mass: f32, friction: f32, shapes: &[Shape]) -> RigidBodyHandle {
    let b = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
    for s in shapes {
        world.add_shape(b, s, Surface { friction, ..Surface::default() }).unwrap();
    }
    world.set_mass(b, mass);
    b
}

#[derive(Clone, Copy)]
pub(crate) struct Variant {
    pub sign: Option<BodyType>,
    pub joints: bool,
    pub collide: bool,
}

/// Base position, base up z, rear position, sign position after each of `steps` steps.
pub(crate) fn trike(v: Variant, steps: usize) -> Vec<(Vector, f32, Vector, Vector)> {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -850.0));
    let slab = world.add_body(BodyType::Static, Pose::IDENTITY);
    world.add_shape(slab, &cuboid([-1000.0, -1000.0, -16.0], [1000.0, 1000.0, 0.0]), Surface::default()).unwrap();
    let sign = v.sign.map(|kind| {
        let b = world.add_body(kind, Pose::IDENTITY);
        world.add_shape(b, &cuboid([-5.0, -50.0, 0.0], [5.0, 50.0, 25.0]), Surface::default()).unwrap();
        b
    });
    let base = body(&mut world, Vector::ZERO, 6.0, 0.8, &[
        cuboid([-9.1, -9.03, 10.17], [9.27, 9.05, 19.17]),
        cuboid([-8.6, -8.81, 1.72], [8.7, 8.84, 11.02]),
        cuboid([-9.88, -5.73, 2.57], [-4.81, 5.73, 6.02]),
        cuboid([7.81, -1.87, 6.06], [10.55, 1.87, 9.65]),
    ]);
    let rear = body(&mut world, Vector::new(-7.276, 0.0, 2.945), 1.0, 0.01, &[wheel(Vector::new(0.0, 7.83, 0.0), 1.55, 3.1), wheel(Vector::new(0.0, -7.93, 0.0), 1.55, 3.1)]);
    let ball = |c: [f32; 3]| [Shape::Sphere { center: Vector::from(c), radius: 1.4 }];
    let right = body(&mut world, Vector::new(5.581, -4.286, 3.385), 1.0, 0.01, &ball([-0.033, 0.0, -1.975]));
    let left = body(&mut world, Vector::new(5.531, 4.289, 3.385), 1.0, 0.01, &ball([0.021, 0.0, -1.975]));
    if v.joints {
        let roll = Rotation::from_rotation_x(FRAC_PI_2);
        let hinges = [
            (rear, [-7.3226, 0.0, 2.9703], [-0.0466, 0.0, 0.0257], roll, 506.5),
            (right, [5.5664, -4.2821, 1.6592], [-0.0149, 0.0037, -1.7258], Rotation::IDENTITY, 9.235),
            (left, [5.5664, 4.3116, 1.6592], [0.0354, 0.0225, -1.7258], Rotation::IDENTITY, 9.235),
        ];
        for (b, at1, at2, frame, max) in hinges {
            let mut j = Joint::new(JointKind::Revolute, Pose::from_parts(Vector::from(at1), frame), Pose::from_parts(Vector::from(at2), frame));
            j.collide = v.collide;
            j.angular_motor = Some(Motor { speed: 0.0, max });
            world.add_joint(base, b, j).unwrap();
        }
    }
    (0..steps)
        .map(|_| {
            world.step(0.02, 2);
            let rb = world.body(base).unwrap();
            let up = (rb.rotation() * Vector::Z).z;
            let s = sign.map_or(Vector::ZERO, |s| world.body(s).unwrap().translation());
            (rb.translation(), up, world.body(rear).unwrap().translation(), s)
        })
        .collect()
}

#[test]
#[ignore = "prints the trike's path for comparing with real's TRIKE markers"]
fn print_trike() {
    let dynamic = Variant { sign: Some(BodyType::Dynamic), joints: true, collide: true };
    let variants = [
        ("dynamic", dynamic),
        ("static", Variant { sign: Some(BodyType::Static), ..dynamic }),
        ("nojoint", Variant { joints: false, ..dynamic }),
        ("nocollide", Variant { collide: false, ..dynamic }),
        ("nosign", Variant { sign: None, ..dynamic }),
        ("nosign-nocollide", Variant { sign: None, collide: false, ..dynamic }),
    ];
    for (name, v) in variants {
        for (i, (b, up, r, s)) in trike(v, 1000).into_iter().enumerate() {
            let t = i + 1;
            if t <= 10 || t <= 100 && t % 5 == 0 || t % 50 == 0 {
                println!("TRIKE {name} t={t} base={:.4},{:.4},{:.4} up={up:.4} rear={:.4},{:.4},{:.4} sign={:.4},{:.4},{:.4}", b.x, b.y, b.z, r.x, r.y, r.z, s.x, s.y, s.z);
            }
        }
    }
}
