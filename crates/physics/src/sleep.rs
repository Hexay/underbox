//! Native's sleep rule, measured with the probe's private worlds (docs/differential.md,
//! divergence 10): a body may sleep while `|v| + max |ω × r|` over the corners `r` of its local
//! bounding box stays under its threshold (inches/s); its island sleeps once all its bodies have
//! been eligible for 0.5 s. Rapier keeps the islands and the timer; this decides eligibility each step.

use crate::world::World;
use rapier3d::math::Vector;
use rapier3d::prelude::{RigidBody, RigidBodyActivation, RigidBodyHandle};

/// managed's `PhysicsBodySettings.SleepThreshold` (inches/s).
pub const DEFAULT_SLEEP_THRESHOLD: f32 = 2.0;

/// Rapier's own test passes when the drift is below this; eligibility is decided before the step.
const ALWAYS: f32 = f32::MAX / 4.0;

impl World {
    /// Let the body sleep when slower than `threshold` (inches/s; 0 means the default), or never.
    pub fn set_auto_sleep(&mut self, id: RigidBodyHandle, enabled: bool, threshold: f32) {
        if enabled {
            let threshold = if threshold > 0.0 { threshold } else { DEFAULT_SLEEP_THRESHOLD };
            self.sleep_thresholds.insert(id, threshold);
        } else {
            self.sleep_thresholds.remove(&id);
            if let Some(rb) = self.body_mut(id) {
                set_eligible(rb, false);
            }
        }
    }

    /// Mark each auto-sleeping body eligible or not from its velocities at the start of the step.
    pub(crate) fn arm_sleep(&mut self) {
        let raw = &mut self.inner;
        for (&id, &threshold) in &self.sleep_thresholds {
            let Some(rb) = raw.bodies.get(id) else { continue };
            // The spin term is never negative, so a body this fast can skip the corners.
            let eligible = rb.linvel().length() < threshold && corner_speed(rb, &raw.colliders) < threshold;
            if let Some(rb) = raw.bodies.get_mut(id) {
                set_eligible(rb, eligible);
            }
        }
    }

    /// Two dynamic bodies that started touching join one island, whose sleep timer starts over:
    /// real's resting three-box stack sleeps one step after a lone box (probe `STACKSLEEP`).
    pub(crate) fn restart_sleep_on_new_contacts(&mut self) {
        let raw = &mut self.inner;
        for &(a, b) in &self.contacts.began {
            let parents = [a, b].map(|c| raw.colliders.get(c).and_then(|c| c.parent()));
            let [Some(pa), Some(pb)] = parents else { continue };
            if !parents.iter().flatten().all(|&p| raw.bodies.get(p).is_some_and(|rb| rb.is_dynamic())) {
                continue;
            }
            for p in [pa, pb] {
                if let Some(rb) = raw.bodies.get_mut(p) {
                    rb.activation_mut().time_since_can_sleep = 0.0;
                }
            }
        }
    }
}

fn set_eligible(rb: &mut RigidBody, eligible: bool) {
    let activation = rb.activation_mut();
    let never = RigidBodyActivation::cannot_sleep();
    activation.normalized_linear_threshold = if eligible { ALWAYS } else { never.normalized_linear_threshold };
    activation.angular_threshold = if eligible { RigidBodyActivation::default_angular_threshold() } else { never.angular_threshold };
}

/// Linear speed plus the fastest spin of a local bounding-box corner about the centre of mass.
fn corner_speed(rb: &RigidBody, colliders: &rapier3d::prelude::ColliderSet) -> f32 {
    let (mut lo, mut hi) = (Vector::splat(f32::MAX), Vector::splat(f32::MIN));
    for c in rb.colliders().iter().filter_map(|&c| colliders.get(c)) {
        let aabb = c.shape().compute_aabb(&c.position_wrt_parent().copied().unwrap_or_default());
        lo = lo.min(aabb.mins);
        hi = hi.max(aabb.maxs);
    }
    let (v, w) = (rb.linvel(), rb.angvel());
    if lo.x > hi.x {
        return v.length();
    }
    let com = rb.local_center_of_mass();
    let rotation = rb.position().rotation;
    let spin = (0..8)
        .map(|i| {
            let corner = Vector::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            w.cross(rotation * (corner - com)).length()
        })
        .fold(0.0, f32::max);
    v.length() + spin
}

#[cfg(test)]
mod tests {
    use crate::shape::{Shape, Surface};
    use crate::world::{BodyType, World};
    use rapier3d::math::{Pose, Rotation, Vector};

    /// The step a free body (gravity off) falls asleep at, 50 Hz, or `None` within 20 s.
    fn sleeps_at(shape: Shape, v: Vector, w: Vector) -> Option<usize> {
        let mut world = World::default();
        world.set_gravity(Vector::ZERO);
        let body = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
        world.add_shape(body, &shape, Surface::default()).unwrap();
        world.set_auto_sleep(body, true, 2.0);
        let rb = world.body_mut(body).unwrap();
        rb.set_linvel(v, true);
        rb.set_angvel(w, true);
        (1..=1000).find(|_| {
            world.step(0.02, 2);
            world.body(body).unwrap().is_sleeping()
        })
    }

    fn ball(radius: f32) -> Shape {
        Shape::Sphere { center: Vector::ZERO, radius }
    }

    fn cuboid(half: Vector) -> Shape {
        Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: half }
    }

    /// Real engine2's private-world results (ProbeRest.Sleep): asleep at step 25, or never.
    #[test]
    fn matches_the_probed_thresholds() {
        let x = |v: f32| Vector::new(v, 0.0, 0.0);
        let z = |w: f32| Vector::new(0.0, 0.0, w);
        let cases = [
            (ball(8.0), x(1.9), Vector::ZERO, true),
            (ball(8.0), x(2.1), Vector::ZERO, false),
            (ball(8.0), Vector::ZERO, z(0.16), true),
            (ball(8.0), Vector::ZERO, z(0.18), false),
            (ball(4.0), Vector::ZERO, z(0.32), true),
            (ball(4.0), Vector::ZERO, z(0.36), false),
            (cuboid(Vector::splat(16.0)), Vector::ZERO, z(0.085), true),
            (cuboid(Vector::splat(16.0)), Vector::ZERO, z(0.095), false),
            (cuboid(Vector::new(4.0, 4.0, 32.0)), Vector::ZERO, z(0.2), true),
            (cuboid(Vector::new(4.0, 4.0, 32.0)), Vector::ZERO, z(0.4), false),
            (cuboid(Vector::new(4.0, 4.0, 32.0)), Vector::ZERO,Vector::new(0.05, 0.0, 0.0), true),
            (cuboid(Vector::new(4.0, 4.0, 32.0)), Vector::ZERO, Vector::new(0.07, 0.0, 0.0), false),
            (ball(8.0), x(1.0), z(0.1), false),
        ];
        for (i, (shape, v, w, sleeps)) in cases.into_iter().enumerate() {
            let at = sleeps_at(shape, v, w);
            assert_eq!(at, sleeps.then_some(25), "case {i}: v {v:?} w {w:?}");
        }
    }

    #[test]
    fn auto_sleep_off_keeps_a_body_awake() {
        let mut world = World::default();
        world.set_gravity(Vector::ZERO);
        let body = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
        world.add_shape(body, &ball(10.0), Surface::default()).unwrap();
        world.set_auto_sleep(body, false, 0.0);
        for _ in 0..300 {
            world.step(1.0 / 60.0, 1);
        }
        assert!(!world.body(body).unwrap().is_sleeping());
        world.set_auto_sleep(body, true, 5.0);
        let slept = (0..300).any(|_| {
            world.step(1.0 / 60.0, 1);
            world.body(body).unwrap().is_sleeping()
        });
        assert!(slept);
    }
}
