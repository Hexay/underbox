//! Restitution on every step a contact approaches, as real engine2 (and Box2D) apply it. Rapier
//! bounces a contact point only on the step it appears, so a pair pushed together every step
//! (orbmatch's bots) never parts. After the step, each point that pushed and approached faster
//! than the threshold at step start gets its normal speed set to `restitution` times that
//! approach (docs/differential-joints.md, orbmatch). Targeting the speed rather than adding to it
//! leaves the points Rapier already bounced alone.

use crate::world::World;
use rapier3d::math::Vector;
use rapier3d::prelude::RigidBody;

/// Approach speed (inches/s) a point must beat to bounce: real bounces 42 but not 40, just above
/// Box2D's 1 m/s (39.37).
const THRESHOLD: f32 = 41.0;

/// A body's velocity at step start.
#[derive(Clone, Copy)]
pub(crate) struct Start {
    linvel: Vector,
    angvel: Vector,
}

/// Velocity of the point at `arm` from the centre of mass.
fn at(linvel: Vector, angvel: Vector, arm: Vector) -> Vector {
    linvel + angvel.cross(arm)
}

/// One body's share of the inverse effective mass along `n` at `arm`.
fn inv_mass_along(rb: &RigidBody, arm: Vector, n: Vector) -> f32 {
    let mp = rb.mass_properties();
    let rn = arm.cross(n);
    n.dot(mp.effective_inv_mass * n) + rn.dot(mp.effective_world_inv_inertia * rn)
}

impl World {
    pub(crate) fn record_step_start(&mut self) {
        self.step_start.clear();
        let awake = self.inner.bodies.iter().filter(|(_, rb)| rb.is_dynamic() && !rb.is_sleeping());
        self.step_start.extend(awake.map(|(h, rb)| (h, Start { linvel: rb.linvel(), angvel: rb.angvel() })));
    }

    pub(crate) fn bounce_approaching_contacts(&mut self) {
        let world = &mut self.inner;
        let starts = &self.step_start;
        for pair in world.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact()) {
            for m in pair.manifolds() {
                let (Some(h1), Some(h2)) = (m.data.rigid_body1, m.data.rigid_body2) else { continue };
                let e = m.data.restitution;
                if e <= 0.0 || !(starts.contains_key(&h1) || starts.contains_key(&h2)) {
                    continue;
                }
                let n = m.data.normal;
                let (Some(rb1), Some(rb2)) = world.bodies.get_pair_mut(h1, h2) else { continue };
                // The solver's lever arms, frozen at the pair's last full narrow-phase update.
                for (a1, a2, acc) in m.points.iter().filter(|p| p.data.impulse > 0.0).map(|p| (p.data.solver_dp1, p.data.solver_dp2, p.data.impulse)) {
                    let start = |h, rb: &RigidBody, arm| starts.get(&h).map_or(at(rb.linvel(), rb.angvel(), arm), |s| at(s.linvel, s.angvel, arm));
                    let approach = n.dot(start(h2, rb2, a2) - start(h1, rb1, a1));
                    if approach > -THRESHOLD {
                        continue;
                    }
                    let k = inv_mass_along(rb1, a1, n) + inv_mass_along(rb2, a2, n);
                    let vn = n.dot(at(rb2.linvel(), rb2.angvel(), a2) - at(rb1.linvel(), rb1.angvel(), a1));
                    // The point's accumulated impulse may shrink to zero, never pull.
                    let impulse = (-(vn + e * approach) / k).max(-acc);
                    rb1.apply_impulse_at_point(-n * impulse, rb1.center_of_mass() + a1, true);
                    rb2.apply_impulse_at_point(n * impulse, rb2.center_of_mass() + a2, true);
                }
            }
        }
    }
}
