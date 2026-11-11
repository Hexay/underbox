//! corpus/orbmatch's rear-end push, headless: two radius-16 spheres, gravity off, each re-aimed at
//! the origin every tick (200 and 300 u/s), against real's `MATCH:POS` markers. Reproduces ours'
//! in-engine numbers exactly; real's differ (docs/differential-joints.md, orbmatch).

use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Vector};

#[test]
#[ignore = "open divergence: real deflects the pair less sideways"]
fn rear_end_push_matches_real() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let ball = |world: &mut World, at: Vector| {
        let b = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
        world.add_shape(b, &Shape::Sphere { center: Vector::ZERO, radius: 16.0 }, Surface::default()).unwrap();
        b
    };
    let front = ball(&mut world, Vector::new(-400.0, 0.0, 32.0));
    let rear = ball(&mut world, Vector::new(-440.0, 8.0, 32.0));
    // The marker logs at the start of tick 20, after 19 steps.
    for _ in 1..20 {
        for (b, speed) in [(front, 200.0), (rear, 300.0)] {
            let rb = world.body_mut(b).unwrap();
            let dir = (-rb.translation()).with_z(0.0).normalize();
            rb.set_linvel(dir * speed, true);
        }
        world.step(0.02, 2);
    }
    let f = world.body(front).unwrap().translation();
    let r = world.body(rear).unwrap().translation();
    assert!((f.x + 310.256).abs() < 0.1 && (f.y + 3.103).abs() < 0.1, "front {f:?}, real (-310.256, -3.103)");
    assert!((r.x + 339.770).abs() < 0.1 && (r.y - 8.969).abs() < 0.1, "rear {r:?}, real (-339.770, 8.969)");
}
