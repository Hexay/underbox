//! Which Rust function answers each slot. See docs/ARCHITECTURE.md#slots.

use crate::generated::slots::{INHERITS, NAMES, SLOT_COUNT};
use std::marker::PhantomData;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Slot `index`, whose implementation has the safe signature `F` (a `fn` pointer type).
/// Constructed only by generated code, so `F` always matches what the thunk at `index` calls.
pub struct Slot<F> {
    pub index: usize,
    _f: PhantomData<F>,
}

impl<F> Slot<F> {
    pub(crate) const fn new(index: usize) -> Self {
        Self { index, _f: PhantomData }
    }

    pub fn name(&self) -> &'static str {
        NAMES[self.index]
    }
}

impl<F> Clone for Slot<F> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F> Copy for Slot<F> {}

#[allow(clippy::declare_interior_mutable_const)]
const ZERO: AtomicUsize = AtomicUsize::new(0);
static IMPLS: [AtomicUsize; SLOT_COUNT] = [ZERO; SLOT_COUNT];
#[allow(clippy::declare_interior_mutable_const)]
const ZERO32: AtomicU32 = AtomicU32::new(0);
static STUB_CALLS: [AtomicU32; SLOT_COUNT] = [ZERO32; SLOT_COUNT];
static OWNERS: OnceLock<Vec<Option<&'static str>>> = OnceLock::new();

/// Collects implementations, then [`Registry::install`]s them once before the table is handed out.
pub struct Registry {
    impls: Vec<usize>,
    owners: Vec<Option<&'static str>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self { impls: vec![0; SLOT_COUNT], owners: vec![None; SLOT_COUNT] }
    }
}

impl Registry {
    /// Answer `slot` with `f`. `owner` is the implementing module (use [`crate::bind!`]).
    pub fn bind<F: Copy + 'static>(&mut self, slot: Slot<F>, f: F, owner: &'static str) {
        const { assert!(size_of::<F>() == size_of::<usize>(), "a slot implementation must be a fn pointer") };
        if let Some(prev) = self.owners[slot.index] {
            panic!("slot {} bound twice ({prev}, then {owner})", slot.name());
        }
        // SAFETY: F is pointer-sized and Copy (a fn pointer); it is read back only as F by this slot's thunk.
        self.impls[slot.index] = unsafe { std::mem::transmute_copy::<F, usize>(&f) };
        self.owners[slot.index] = Some(owner);
    }

    /// As [`Registry::bind`], unless something already answers `slot` (generated defaults).
    pub fn bind_default<F: Copy + 'static>(&mut self, slot: Slot<F>, f: F, owner: &'static str) {
        if self.owners[slot.index].is_none() {
            self.bind(slot, f, owner);
        }
    }

    /// Publish every binding. An inherited slot left unbound takes its declaring class's binding:
    /// its safe signature differs only in `NativeRef`'s marker type, which is layout-identical.
    pub fn install(mut self) {
        for &(derived, base) in INHERITS {
            if self.impls[derived] == 0 && self.impls[base] != 0 {
                self.impls[derived] = self.impls[base];
                self.owners[derived] = self.owners[base];
            }
        }
        if OWNERS.set(self.owners).is_err() {
            panic!("Registry::install called twice");
        }
        for (slot, f) in IMPLS.iter().zip(self.impls) {
            slot.store(f, Ordering::Release);
        }
    }
}

/// Bind a slot to a function, recording the calling module as its owner.
#[macro_export]
macro_rules! bind {
    ($registry:expr, $slot:expr, $f:expr) => {
        $registry.bind($slot, $f, module_path!())
    };
}

/// The bound implementation of slot `index`, as the thunk's signature `F`.
#[inline]
pub(crate) fn implementation<F: Copy>(index: usize) -> Option<F> {
    let p = IMPLS[index].load(Ordering::Acquire);
    // SAFETY: only `Registry::bind::<F>` for this index stores here, and the thunk asks for the same F.
    (p != 0).then(|| unsafe { std::mem::transmute_copy::<usize, F>(&p) })
}

/// An unbound slot was called: count it and log the first call.
#[cold]
pub(crate) fn stub_hit(index: usize) {
    if STUB_CALLS[index].fetch_add(1, Ordering::Relaxed) == 0 {
        crate::rt::log::warn(&format!("stub: {} (slot {index})", NAMES[index]));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    Stub,
    Bound { owner: &'static str },
}

pub fn status(index: usize) -> SlotStatus {
    match OWNERS.get().and_then(|o| o[index]) {
        Some(owner) => SlotStatus::Bound { owner },
        None => SlotStatus::Stub,
    }
}

/// How often each unbound slot was called since start: what the running game still needs.
pub fn stub_calls() -> impl Iterator<Item = (usize, u32)> {
    STUB_CALLS.iter().enumerate().map(|(i, c)| (i, c.load(Ordering::Relaxed))).filter(|&(_, c)| c > 0)
}
