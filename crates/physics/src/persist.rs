//! Real engine2 refreshes a persistent contact once either body has turned 10° since the pair's
//! last full update; Rapier's recycling waits for 11.5° (`cos Δθ > 0.98`, hard-coded). Before each
//! step, a pair past real's bound gets its first shape's hooks switched on for that step, which
//! makes Rapier update it in full (docs/differential-joints.md, 18).

use crate::dense::Dense;
use crate::world::World;
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{ActiveHooks, ColliderHandle, ColliderSet, ContactPair};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// cos 10°: real refreshes a rolling pair at 10.4° of turn and keeps it at 9.4° (probe `REFRESH
/// friction`).
const MAX_TURN_COS: f32 = 0.984_807_8;

/// A pair's state at its last full update.
#[derive(Clone, Copy)]
struct Frozen {
    /// The solver normal and first lever arm, which recycled steps keep bit for bit.
    sig: (Vector, Vector),
    rot1: Rotation,
    rot2: Rotation,
    /// The step parity this entry was last kept on; the others are dropped after the pass.
    seen: bool,
}

/// Every active, unhooked pair's [`Frozen`] state, updated in place each step: rebuilding it into
/// a second map wrote a cold copy every step (docs/perf-bandwidth.md, "Pile: physics working set").
#[derive(Default)]
pub(crate) struct FrozenPairs {
    pairs: FxHashMap<(ColliderHandle, ColliderHandle), Frozen>,
    parity: bool,
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

fn unhooked(colliders: &ColliderSet, pair: &ContactPair) -> bool {
    let hooks = |h| colliders.get(h).map_or(ActiveHooks::all(), |c| c.active_hooks());
    (hooks(pair.collider1) | hooks(pair.collider2)).is_empty()
}

/// Per-step bookkeeping: moving shapes' orientations at step start (a full update runs at those),
/// and the shapes whose hooks were switched on to force one.
#[derive(Default)]
pub(crate) struct StepStart {
    /// `Rotation`'s components: the SIMD quaternion pads a slot to 48 bytes, this to 24.
    rotations: Dense<[f32; 4]>,
    forced: Vec<ColliderHandle>,
}

impl StepStart {
    fn rotation(&self, colliders: &ColliderSet, id: ColliderHandle) -> Rotation {
        self.rotations.get(id.into_raw_parts()).map_or_else(|| colliders.get(id).map_or(Rotation::IDENTITY, |c| c.position().rotation), Rotation::from_array)
    }
}

impl World {
    pub(crate) fn force_turned_contacts(&mut self) -> StepStart {
        let mut start = std::mem::take(&mut self.step_scratch);
        start.rotations.clear();
        start.forced.clear();
        let moving = self.inner.bodies.iter().filter(|(_, rb)| !rb.is_fixed() && !rb.is_sleeping());
        for h in moving.flat_map(|(_, rb)| rb.colliders().iter().copied()) {
            if let Some(c) = self.inner.colliders.get(h) {
                start.rotations.insert(h.into_raw_parts(), c.position().rotation.to_array());
            }
        }
        let colliders = &self.inner.colliders;
        for pair in self.inner.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact() && unhooked(colliders, p)) {
            let Some(f) = self.frozen.pairs.get(&(pair.collider1, pair.collider2)) else { continue };
            let (r1, r2) = (start.rotation(colliders, pair.collider1), start.rotation(colliders, pair.collider2));
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

    /// Switch the forced shapes' hooks back after the step.
    pub(crate) fn restore_forced_hooks(&mut self, start: &StepStart) {
        for &id in &start.forced {
            self.refresh_hooks(id);
        }
    }
}

impl FrozenPairs {
    /// Start a step's pass: pairs not [`Self::keep`]-ed before [`Self::end`] are dropped.
    pub(crate) fn begin(&mut self) {
        self.parity = !self.parity;
    }

    /// Record an active pair's state after the step, unless it's recycled unchanged.
    pub(crate) fn keep(&mut self, colliders: &ColliderSet, start: &StepStart, pair: &ContactPair) {
        if !unhooked(colliders, pair) {
            return;
        }
        let Some(sig) = signature(pair) else { return };
        let seen = self.parity;
        let forced = start.forced.contains(&pair.collider1) || start.forced.contains(&pair.collider2);
        let fresh = || Frozen { sig, rot1: start.rotation(colliders, pair.collider1), rot2: start.rotation(colliders, pair.collider2), seen };
        match self.pairs.entry((pair.collider1, pair.collider2)) {
            Entry::Occupied(mut kept) if kept.get().sig == sig && !forced => kept.get_mut().seen = seen,
            Entry::Occupied(mut stale) => *stale.get_mut() = fresh(),
            Entry::Vacant(new) => {
                new.insert(fresh());
            }
        }
    }

    pub(crate) fn end(&mut self) {
        let seen = self.parity;
        self.pairs.retain(|_, f| f.seen == seen);
    }
}
