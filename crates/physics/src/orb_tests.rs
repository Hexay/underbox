//! corpus/orbmatch's rear-end push, headless: two radius-16 spheres at (-400, 0) and (-440, y),
//! gravity off, each re-aimed at the origin every step (200 u/s and `rear` u/s), against real's
//! probe `REAIM` markers (tools/diff/probe/Code/ProbePush.cs; docs/differential-joints.md, 16).

use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Vector};

/// After each step: front and rear positions, then velocities.
fn push(y: f32, rear: f32, friction: f32, steps: usize) -> Vec<[Vector; 4]> {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let ball = |world: &mut World, at: Vector| {
        let b = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
        world.add_shape(b, &Shape::Sphere { center: Vector::ZERO, radius: 16.0 }, Surface { friction, ..Surface::default() }).unwrap();
        b
    };
    let bodies = [ball(&mut world, Vector::new(-400.0, 0.0, 32.0)), ball(&mut world, Vector::new(-440.0, y, 32.0))];
    (0..steps)
        .map(|_| {
            for (b, speed) in bodies.into_iter().zip([200.0, rear]) {
                let rb = world.body_mut(b).unwrap();
                let dir = (-rb.translation()).with_z(0.0).normalize();
                rb.set_linvel(dir * speed, true);
            }
            world.step(0.02, 2);
            let [f, r] = bodies.map(|b| world.body(b).unwrap().clone());
            [f.translation(), r.translation(), f.linvel(), r.linvel()]
        })
        .collect()
}

/// `(step, [front, rear, front velocity, rear velocity])` in x, y against what `push` gives.
fn off(got: &[[Vector; 4]], real: &[(usize, [[f32; 2]; 4])], tol: f32) -> Vec<String> {
    let mut off = Vec::new();
    for (t, want) in real {
        for (i, (g, w)) in got[t - 1].iter().zip(want).enumerate() {
            if (g.x - w[0]).abs() > tol || (g.y - w[1]).abs() > tol {
                off.push(format!("t={t} #{i} ({:.4}, {:.4}), real ({}, {})", g.x, g.y, w[0], w[1]));
            }
        }
    }
    off
}

#[test]
fn rear_end_push_matches_real() {
    let real = [
        (10, [[-354.9841, -1.1436], [-385.0263, 8.0464], [250.2174, -12.0144], [249.7201, 6.4265]]),
        (19, [[-310.2564, -3.1029], [-339.7695, 8.9692], [250.5523, -10.7957], [249.3397, 4.9087]]),
    ];
    let off = off(&push(8.0, 300.0, Surface::default().friction, 19), &real, 0.05);
    assert!(off.is_empty(), "{off:#?}");
}

#[test]
fn frictionless_rear_end_push_matches_real() {
    let real = [
        (10, [[-354.9986, -1.2155], [-385.012, 8.1172], [250.2384, -11.9371], [249.6978, 6.334]]),
        (19, [[-310.6064, -3.8206], [-339.4209, 9.6745], [246.9611, -16.7966], [252.9131, 10.7678]]),
    ];
    let off = off(&push(8.0, 300.0, 0.0, 19), &real, 0.05);
    assert!(off.is_empty(), "{off:#?}");
}

/// Real bounces a pair pushed together on every step it approaches, not just the first, when
/// the approach beats its threshold: a 42 u/s approach bounces, 40 doesn't (`REAIM y=0 rear=`).
#[test]
fn a_pushed_pair_bounces_every_step_above_the_threshold() {
    for (rear, front_after) in [(300.0, 252.5), (242.0, 222.05), (240.0, 220.0)] {
        let got = push(0.0, rear, Surface::default().friction, 20);
        let f = got[19][2].x;
        assert!((f - front_after).abs() < 0.01, "rear {rear}: front ends at {f}, real {front_after}");
    }
    let got = push(0.0, 300.0, Surface::default().friction, 20);
    for (t, [.., f, r]) in got.iter().enumerate().skip(4) {
        assert!((f.x - 252.5).abs() < 0.01 && (r.x - 247.5).abs() < 0.01, "t={}: {f:?} {r:?}, real 252.5 and 247.5", t + 1);
    }
}
