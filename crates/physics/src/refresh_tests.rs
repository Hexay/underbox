//! When a persistent contact is refreshed, headless, against real's probe `REFRESH` markers
//! (tools/diff/probe/Code/ProbeRefresh.cs; docs/differential-joints.md, 18): ball B pushed into a
//! ball A held still before every step, so A's velocity after the step points along the normal used.

use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Vector};

/// The angle (degrees, in xy) of A's velocity after each step.
fn slide(radius: f32, vt: f32, friction: f32, steps: usize) -> Vec<f32> {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let ball = |world: &mut World, at: Vector| {
        let b = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
        world.add_shape(b, &Shape::Sphere { center: Vector::ZERO, radius }, Surface { friction, ..Surface::default() }).unwrap();
        b
    };
    let a = ball(&mut world, Vector::ZERO);
    let b = ball(&mut world, Vector::new(-2.0 * radius - 1.0, 0.0, 0.0));
    (0..steps)
        .map(|_| {
            world.body_mut(a).unwrap().set_linvel(Vector::ZERO, true);
            world.body_mut(b).unwrap().set_linvel(Vector::new(100.0, vt, 0.0), true);
            world.step(0.02, 2);
            let v = world.body(a).unwrap().linvel();
            v.y.atan2(v.x).to_degrees()
        })
        .collect()
}

/// Rolling balls turn together, so only the bodies' own turn refreshes the pair: real at 10.4°
/// (step 13) but not 9.4°, where Rapier waits for 11.5° (step 15).
#[test]
fn a_rolling_pair_refreshes_after_ten_degrees_of_turn() {
    let real = [(12, -0.9240), (13, -8.3410), (19, -11.0565), (20, -18.7598), (24, -21.7753), (25, -29.2680), (29, -39.6371)];
    let got = slide(16.0, 25.0, Surface::default().friction, 30);
    for (t, want) in real {
        let n = got[t - 1];
        assert!((n - want).abs() < 0.1, "t={t}: normal at {n}°, real {want}°");
    }
}

/// Sliding without friction, a pair refreshes once it has drifted about 1.97 units (real and ours).
#[test]
fn a_sliding_pair_refreshes_after_two_units_of_drift() {
    let got = slide(16.0, 25.0, 0.0, 22);
    let jumps: Vec<_> = (2..got.len()).filter(|&i| (got[i] - got[i - 1]).abs() > 0.05).map(|i| i + 1).collect();
    assert_eq!(jumps, [6, 9, 12, 15, 17, 19, 21]);
}
