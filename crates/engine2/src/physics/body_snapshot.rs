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

/// What the hot getters answer, exactly as they'd read it from the locked body.
#[derive(Clone, Copy)]
pub struct State {
    pub translation: Vector,
    pub rotation: Rotation,
    pub linvel: Vector,
    pub angvel: Vector,
    /// Real reports static bodies asleep.
    pub asleep: bool,
}

impl State {
    pub fn of(rb: &RigidBody) -> Self {
        Self { translation: rb.translation(), rotation: *rb.rotation(), linvel: rb.linvel(), angvel: rb.angvel(), asleep: rb.is_fixed() || rb.is_sleeping() }
    }

    fn to_bits(self) -> [u32; 14] {
        let (t, q, l, a) = (self.translation, self.rotation, self.linvel, self.angvel);
        [t.x, t.y, t.z, q.x, q.y, q.z, q.w, l.x, l.y, l.z, a.x, a.y, a.z, f32::from_bits(self.asleep as u32)].map(f32::to_bits)
    }

    fn from_bits(b: [u32; 14]) -> Self {
        let f = b.map(f32::from_bits);
        Self {
            translation: Vector::new(f[0], f[1], f[2]),
            rotation: Rotation::from_xyzw(f[3], f[4], f[5], f[6]),
            linvel: Vector::new(f[7], f[8], f[9]),
            angvel: Vector::new(f[10], f[11], f[12]),
            asleep: b[13] != 0,
        }
    }
}

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

/// Make every snapshot of the world stale. Call with the world locked, before changing anything.
pub fn invalidate(world_epoch: &AtomicU64) {
    world_epoch.fetch_add(1, Ordering::Relaxed);
    fence(Ordering::Release);
}

/// Record `rb`'s state as current. Call with the world locked, after its last body change.
pub fn publish(world_epoch: &AtomicU64, snapshot: &Snapshot, rb: &RigidBody) {
    for (slot, bits) in snapshot.bits.iter().zip(State::of(rb).to_bits()) {
        slot.store(bits, Ordering::Relaxed);
    }
    snapshot.epoch.store(world_epoch.load(Ordering::Relaxed), Ordering::Release);
}

/// The published state, if nothing that could change a body locked the world since.
pub fn read(world_epoch: &AtomicU64, snapshot: &Snapshot) -> Option<State> {
    let epoch = world_epoch.load(Ordering::Acquire);
    if snapshot.epoch.load(Ordering::Acquire) != epoch {
        return None;
    }
    let bits = snapshot.bits.each_ref().map(|b| b.load(Ordering::Relaxed));
    fence(Ordering::Acquire);
    (world_epoch.load(Ordering::Relaxed) == epoch).then(|| State::from_bits(bits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use physics::{BodyType, Pose, World};

    #[test]
    fn a_snapshot_reads_back_until_the_world_changes() {
        let mut world = World::default();
        let id = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(1.0, 2.0, 3.0)));
        world.body_mut(id).unwrap().set_linvel(Vector::new(4.0, 5.0, 6.0), true);
        let (epoch, snap) = (AtomicU64::new(0), Snapshot::default());
        assert!(read(&epoch, &snap).is_none());
        invalidate(&epoch);
        publish(&epoch, &snap, world.body(id).unwrap());
        let s = read(&epoch, &snap).unwrap();
        let want = State::of(world.body(id).unwrap());
        assert_eq!(s.to_bits(), want.to_bits());
        assert_eq!(s.translation, Vector::new(1.0, 2.0, 3.0));
        assert!(!s.asleep);
        invalidate(&epoch);
        assert!(read(&epoch, &snap).is_none());
    }

    #[test]
    fn getters_see_a_change_made_after_the_step() {
        use super::super::{body, body_cache, events, world};
        crate::handles::fake_managed();
        let w: world::Ref = objects::create_shared(std::sync::Arc::new(world::PhysicsWorld::default()));
        let settings = abi::structs::PhysicsBodySettings { flags: body::GRAVITY | (1 << 2), ..Default::default() };
        let b = body::spawn(w, BodyType::Dynamic, Pose::IDENTITY, &settings).unwrap();
        let published = || body_cache::resolve(b, |e| read(&e.world.epoch, &e.body.snapshot).is_some()).unwrap();
        let (active, _) = events::simulate(w, 0.02, 1);
        assert_eq!(active.len(), 1);
        assert!(published(), "the step publishes an awake body");
        let locked = body::read_rb(b, |rb| Some(State::of(rb).to_bits()));
        assert_eq!(body::read_state(b).map(|s| s.to_bits()), locked);
        body::with_rb(b, |rb| rb.set_linvel(Vector::new(7.0, 0.0, 0.0), true));
        assert!(!published(), "a change makes it stale");
        assert_eq!(body::read_state(b).unwrap().linvel, Vector::new(7.0, 0.0, 0.0));
    }
}
