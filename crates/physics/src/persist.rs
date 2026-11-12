//! Real engine2 refreshes a persistent contact once either body has turned 10° since the pair's
//! last full update; Rapier's recycling waits for 11.5° (`cos Δθ > 0.98`, hard-coded). Before each
//! step, a pair past real's bound gets its first shape's hooks switched on for that step, which
//! makes Rapier update it in full (docs/differential-joints.md, 18).

use crate::world::World;
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{ActiveHooks, ColliderHandle, ContactPair};
use std::collections::HashMap;

/// cos 10°: real refreshes a rolling pair at 10.4° of turn and keeps it at 9.4° (probe `REFRESH
/// friction`).
const MAX_TURN_COS: f32 = 0.984_807_8;

/// A pair's state at its last full update.
#[derive(Clone, Copy)]
pub(crate) struct Frozen {
    /// The solver normal and first lever arm, which recycled steps keep bit for bit.
    sig: (Vector, Vector),
    rot1: Rotation,
    rot2: Rotation,
}

fn signature(pair: &ContactPair) -> Option<(Vector, Vector)> {
    let m = pair.manifolds().iter().find(|m| !m.points.is_empty())?;
    Some((m.data.normal, m.points[0].data.solver_dp1))
}

/// `cos Δθ` between two orientations, as Rapier's recycling measures it.
fn turn_cos(a: Rotation, b: Rotation) -> f32 {
    let c = a.dot(b);
    2.0 * c * c - 1.0
}

/// Per-step bookkeeping: moving shapes' orientations at step start (a full update runs at those),
/// and the shapes whose hooks were switched on to force one.
#[derive(Default)]
pub(crate) struct StepStart {
    rotations: HashMap<ColliderHandle, Rotation>,
    forced: Vec<ColliderHandle>,
}

impl StepStart {
    fn rotation(&self, world: &World, id: ColliderHandle) -> Rotation {
        self.rotations.get(&id).copied().unwrap_or_else(|| world.inner.colliders.get(id).map_or(Rotation::IDENTITY, |c| c.position().rotation))
    }
}

impl World {
    fn unhooked(&self, pair: &ContactPair) -> bool {
        let hooks = |h| self.inner.colliders.get(h).map_or(ActiveHooks::all(), |c| c.active_hooks());
        (hooks(pair.collider1) | hooks(pair.collider2)).is_empty()
    }

    pub(crate) fn force_turned_contacts(&mut self) -> StepStart {
        let mut start = StepStart::default();
        let moving = self.inner.bodies.iter().filter(|(_, rb)| !rb.is_fixed() && !rb.is_sleeping());
        let shapes = moving.flat_map(|(_, rb)| rb.colliders().iter().copied());
        start.rotations = shapes.filter_map(|h| Some((h, self.inner.colliders.get(h)?.position().rotation))).collect();
        for pair in self.inner.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact() && self.unhooked(p)) {
            let Some(f) = self.frozen.get(&(pair.collider1, pair.collider2)) else { continue };
            let (r1, r2) = (start.rotation(self, pair.collider1), start.rotation(self, pair.collider2));
            if turn_cos(f.rot1, r1).min(turn_cos(f.rot2, r2)) <= MAX_TURN_COS {
                start.forced.push(pair.collider1);
            }
        }
        for &id in &start.forced {
            if let Some(c) = self.inner.colliders.get_mut(id) {
                c.set_active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS);
            }
        }
        start
    }

    pub(crate) fn release_forced_contacts(&mut self, start: StepStart) {
        for &id in &start.forced {
            self.refresh_hooks(id);
        }
        let mut frozen = HashMap::with_capacity(self.frozen.len());
        for pair in self.inner.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact() && self.unhooked(p)) {
            let key = (pair.collider1, pair.collider2);
            let Some(sig) = signature(pair) else { continue };
            let forced = start.forced.contains(&pair.collider1) || start.forced.contains(&pair.collider2);
            let entry = match self.frozen.get(&key).filter(|f| f.sig == sig && !forced) {
                Some(f) => *f,
                None => Frozen { sig, rot1: start.rotation(self, pair.collider1), rot2: start.rotation(self, pair.collider2) },
            };
            frozen.insert(key, entry);
        }
        self.frozen = frozen;
    }
}
