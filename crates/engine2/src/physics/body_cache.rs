//! Per-thread resolution of body refs to their objects, so a body slot skips the registry's lock,
//! its lookup, a `Weak` upgrade and the refcount traffic (docs/perf-physics.md, round 4). Entries
//! hold strong refs and are valid until any body is deleted: `forget` bumps `EPOCH` after the
//! registry dropped the body, and a thread whose entries carry an older epoch drops them all on
//! its next call.

use super::body::{Body, Ref};
use super::world::PhysicsWorld;
use physics::BodyId;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static EPOCH: AtomicU64 = AtomicU64::new(0);

/// A resolved body. `id` is copied out of `body` so the hot getters leave `body` untouched.
pub struct Entry {
    pub id: BodyId,
    pub world: Arc<PhysicsWorld>,
    pub body: Arc<Body>,
}

#[derive(Default)]
struct Resolved {
    epoch: u64,
    bodies: FxHashMap<usize, Entry>,
}

thread_local! {
    static RESOLVED: RefCell<Resolved> = RefCell::default();
}

fn lookup(r: Ref) -> Option<Entry> {
    let body = super::body::get(r)?;
    let world = body.sim_world.upgrade()?;
    Some(Entry { id: body.id, world, body })
}

/// Run `f` on a live body and its world.
pub fn resolve<R>(r: Ref, f: impl FnOnce(&Entry) -> R) -> Option<R> {
    let epoch = EPOCH.load(Ordering::Acquire);
    let mut f = Some(f);
    let cached = RESOLVED.try_with(|cell| {
        // Gotcha: `f` may resolve another body, so a hit runs it under a shared borrow, and a
        // miss inside it (no mutable borrow to be had) isn't stored.
        if let Ok(c) = cell.try_borrow()
            && c.epoch == epoch
            && let Some(entry) = c.bodies.get(&r.addr())
        {
            return Some(f.take()?(entry));
        }
        let entry = lookup(r)?;
        if let Ok(mut c) = cell.try_borrow_mut() {
            if c.epoch != epoch {
                c.bodies.clear();
                c.epoch = epoch;
            }
            let copy = Entry { id: entry.id, world: entry.world.clone(), body: entry.body.clone() };
            c.bodies.insert(r.addr(), copy);
        }
        Some(f.take()?(&entry))
    });
    match (cached, f) {
        (Ok(done), _) => done,
        (Err(_), Some(f)) => lookup(r).map(|e| f(&e)),
        (Err(_), None) => None,
    }
}

/// A body is gone: every thread's entries are stale. Call after the registry dropped it.
pub fn forget() {
    EPOCH.fetch_add(1, Ordering::Release);
    let _ = RESOLVED.try_with(|cell| cell.try_borrow_mut().map(|mut c| c.bodies.clear()));
}
