//! Lock-free reads of a body's pose, velocities and sleep state between steps, for the getters
//! managed calls on every body every tick (docs/perf-physics.md, round 5). The world mutex costs
//! two locked instructions per call, and they stall on managed's pending stores.
//!
//! A step publishes each active body's state, tagged with the world's epoch, while it still holds
//! the world lock. Every lock that may change a body bumps the epoch first
//! (`PhysicsWorld::sim_mut`), which makes all snapshots stale at once; readers then take the lock.
//! It's a seqlock: a reader checks the tags before its loads and the epoch again after them, so a
//! publish racing it is detected, never half-read.

use physics::{RigidBody, Rotation, Vector};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

/// What the per-tick getters read, from a snapshot or the locked body alike. An enum, not a trait
/// object: an out-of-line accessor returned its vector through the stack, and reloading it in
/// other widths stalled on managed's pending stores as the lock did.
#[derive(Clone, Copy)]
pub enum BodyRead<'a> {
    Snapshot(&'a Snapshot),
    Body(&'a RigidBody),
}

impl BodyRead<'_> {
    #[inline(always)]
    pub fn translation(self) -> Vector {
        match self {
            Self::Snapshot(s) => s.vector(TRANSLATION),
            Self::Body(rb) => rb.translation(),
        }
    }

    #[inline(always)]
    pub fn rotation(self) -> Rotation {
        match self {
            Self::Snapshot(s) => Rotation::from_xyzw(s.f(ROTATION), s.f(ROTATION + 1), s.f(ROTATION + 2), s.f(ROTATION + 3)),
            Self::Body(rb) => *rb.rotation(),
        }
    }

    #[inline(always)]
    pub fn linvel(self) -> Vector {
        match self {
            Self::Snapshot(s) => s.vector(LINVEL),
            Self::Body(rb) => rb.linvel(),
        }
    }

    #[inline(always)]
    pub fn angvel(self) -> Vector {
        match self {
            Self::Snapshot(s) => s.vector(ANGVEL),
            Self::Body(rb) => rb.angvel(),
        }
    }

    /// Real reports static bodies asleep.
    #[inline(always)]
    pub fn asleep(self) -> bool {
        match self {
            Self::Snapshot(s) => s.bits[ASLEEP].load(Ordering::Relaxed) != 0,
            Self::Body(rb) => rb.is_fixed() || rb.is_sleeping(),
        }
    }
}

const TRANSLATION: usize = 0;
const ROTATION: usize = 3;
const LINVEL: usize = 7;
const ANGVEL: usize = 10;
const ASLEEP: usize = 13;

#[repr(align(64))]
pub struct Snapshot {
    epoch: AtomicU64,
    bits: [AtomicU32; 14],
}

impl Default for Snapshot {
    fn default() -> Self {
        // Never matches a world epoch until published.
        Self { epoch: AtomicU64::new(u64::MAX), bits: Default::default() }
    }
}

impl Snapshot {
    #[inline(always)]
    fn f(&self, i: usize) -> f32 {
        f32::from_bits(self.bits[i].load(Ordering::Relaxed))
    }

    #[inline(always)]
    fn vector(&self, at: usize) -> Vector {
        Vector::new(self.f(at), self.f(at + 1), self.f(at + 2))
    }
}

/// Make every snapshot of the world stale. Call with the world locked, before changing anything.
pub fn invalidate(world_epoch: &AtomicU64) {
    world_epoch.fetch_add(1, Ordering::Relaxed);
    fence(Ordering::Release);
}

/// Record `rb`'s state as current. Call with the world locked, after its last body change.
pub fn publish(world_epoch: &AtomicU64, snapshot: &Snapshot, rb: &RigidBody) {
    let b = BodyRead::Body(rb);
    let (t, q, l, a) = (b.translation(), b.rotation(), b.linvel(), b.angvel());
    let values = [t.x, t.y, t.z, q.x, q.y, q.z, q.w, l.x, l.y, l.z, a.x, a.y, a.z, f32::from_bits(b.asleep() as u32)];
    for (slot, v) in snapshot.bits.iter().zip(values) {
        slot.store(v.to_bits(), Ordering::Relaxed);
    }
    snapshot.epoch.store(world_epoch.load(Ordering::Relaxed), Ordering::Release);
}

/// `f` of the published state, if nothing that could change a body locked the world since.
#[inline(always)]
pub fn read<R>(world_epoch: &AtomicU64, snapshot: &Snapshot, f: impl FnOnce(BodyRead) -> R) -> Option<R> {
    let epoch = world_epoch.load(Ordering::Acquire);
    if snapshot.epoch.load(Ordering::Acquire) != epoch {
        return None;
    }
    let value = f(BodyRead::Snapshot(snapshot));
    fence(Ordering::Acquire);
    (world_epoch.load(Ordering::Relaxed) == epoch).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use physics::{BodyType, Pose, World};

    fn all(b: BodyRead) -> (Vector, Rotation, Vector, Vector, bool) {
        (b.translation(), b.rotation(), b.linvel(), b.angvel(), b.asleep())
    }

    #[test]
    fn a_snapshot_reads_back_until_the_world_changes() {
        let mut world = World::default();
        let id = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(1.0, 2.0, 3.0)));
        world.body_mut(id).unwrap().set_linvel(Vector::new(4.0, 5.0, 6.0), true);
        let (epoch, snap) = (AtomicU64::new(0), Snapshot::default());
        assert!(read(&epoch, &snap, all).is_none());
        invalidate(&epoch);
        publish(&epoch, &snap, world.body(id).unwrap());
        assert_eq!(read(&epoch, &snap, all), Some(all(BodyRead::Body(world.body(id).unwrap()))));
        invalidate(&epoch);
        assert!(read(&epoch, &snap, all).is_none());
    }

    #[test]
    fn getters_see_a_change_made_after_the_step() {
        use super::super::{body, body_cache, events, world};
        crate::handles::fake_managed();
        let w: world::Ref = objects::create_shared(std::sync::Arc::new(world::PhysicsWorld::default()));
        let settings = abi::structs::PhysicsBodySettings { flags: body::GRAVITY | (1 << 2), ..Default::default() };
        let b = body::spawn(w, BodyType::Dynamic, Pose::IDENTITY, &settings).unwrap();
        let published = || body_cache::resolve(b, |e| read(&e.world.epoch, &e.body.snapshot, |_| ()).is_some()).unwrap();
        let (active, _) = events::simulate(w, 0.02, 1);
        assert_eq!(active.len(), 1);
        assert!(published(), "the step publishes an awake body");
        assert_eq!(body::read_hot(b, |s| Some(all(s))), body::read_rb(b, |rb| Some(all(BodyRead::Body(rb)))));
        body::with_rb(b, |rb| rb.set_linvel(Vector::new(7.0, 0.0, 0.0), true));
        assert!(!published(), "a change makes it stale");
        assert_eq!(body::read_hot(b, |s| s.linvel()), Vector::new(7.0, 0.0, 0.0));
    }
}
