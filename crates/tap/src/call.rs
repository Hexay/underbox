//! One logged call: the generated trampolines open it, add arguments, and close it with the return
//! value, which writes one record (docs/differential-tap.md, "Records").

use crate::config;
use crate::generated::trampolines::CALLBACK_NAMES;
use crate::sink;
use crate::val::{Line, Val};
use std::cell::Cell;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// A slot: managed calls native.
    Native,
    /// A callback: native calls managed.
    Managed,
}

static NATIVE: [AtomicUsize; abi::SLOT_COUNT] = [const { AtomicUsize::new(0) }; abi::SLOT_COUNT];
static MANAGED: [AtomicUsize; abi::CALLBACK_COUNT] = [const { AtomicUsize::new(0) }; abi::CALLBACK_COUNT];
static SEQ: AtomicU64 = AtomicU64::new(0);
static TICK: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// The target engine's function for slot `i`.
pub fn native(i: usize) -> usize {
    NATIVE[i].load(Relaxed)
}

/// Managed's function for callback `i`.
pub fn managed(i: usize) -> usize {
    MANAGED[i].load(Relaxed)
}

/// Keep managed's callbacks and return the table to hand the target instead.
///
/// # Safety
/// `table` holds `CALLBACK_COUNT` function pointers.
pub unsafe fn wrap_managed(table: *const usize) -> *const usize {
    static OURS: std::sync::OnceLock<[usize; abi::CALLBACK_COUNT]> = std::sync::OnceLock::new();
    let theirs = unsafe { std::slice::from_raw_parts(table, abi::CALLBACK_COUNT) };
    for (slot, &f) in MANAGED.iter().zip(theirs) {
        slot.store(f, Relaxed);
    }
    OURS.get_or_init(crate::generated::trampolines::callback_trampolines).as_ptr()
}

/// Keep the target's slot functions and point managed's table at the trampolines. Returns how many
/// entries the target left null (those stay null).
///
/// # Safety
/// `table` holds `SLOT_COUNT` writable entries.
pub unsafe fn wrap_native(table: *mut usize) -> usize {
    let table = unsafe { std::slice::from_raw_parts_mut(table, abi::SLOT_COUNT) };
    let ours = crate::generated::trampolines::slot_trampolines();
    let mut null = 0;
    for ((entry, slot), t) in table.iter_mut().zip(&NATIVE).zip(ours) {
        slot.store(*entry, Relaxed);
        if *entry == 0 {
            null += 1;
        } else {
            *entry = t;
        }
    }
    null
}

pub struct Call {
    side: Side,
    index: usize,
    seq: u64,
    tick: u64,
    depth: u32,
    line: Line,
}

impl Call {
    #[inline]
    pub fn begin(side: Side, index: usize) -> Option<Call> {
        let cfg = config::get()?;
        if cfg.tick == Some((side, index)) {
            TICK.fetch_add(1, Relaxed);
            sink::flush_all();
        }
        if !cfg.enabled(side, index) || sink::full() {
            return None;
        }
        let tick = TICK.load(Relaxed);
        if tick < cfg.ticks.0 || tick > cfg.ticks.1 {
            return None;
        }
        let depth = DEPTH.with(|d| d.replace(d.get() + 1));
        Some(Call { side, index, seq: SEQ.fetch_add(1, Relaxed), tick, depth, line: Line::default() })
    }

    pub fn val<T: Val>(&mut self, v: &T) {
        self.line.arg(v);
    }

    /// A value managed passes by reference, or an out/ref argument after the call.
    pub fn by_ref<T: Val + Copy>(&mut self, p: *const T) {
        self.line.next();
        if p.is_null() {
            self.line.raw("null");
        } else {
            // SAFETY: a by-reference argument points at a live `T` for the call.
            unsafe { p.read_unaligned() }.put(&mut self.line);
        }
    }

    pub fn bytes(&mut self, p: *const u8, n: usize) {
        self.line.next();
        self.line.bytes(p, n);
    }

    pub fn outs(&mut self) {
        self.line.bar();
    }

    /// Write the record.
    pub fn ret<T: Val>(self, r: &T) {
        DEPTH.with(|d| d.set(self.depth));
        let (side, name) = match self.side {
            Side::Native => ('N', abi::NAMES[self.index]),
            Side::Managed => ('M', CALLBACK_NAMES[self.index]),
        };
        let mut rec = String::with_capacity(self.line.buf.len() + name.len() + 48);
        write!(rec, "{}\t{}\t{}\t{side}\t{name}\t{}\t", self.seq, self.tick, self.depth, self.line.buf).unwrap();
        let mut l = Line::default();
        l.buf = rec;
        r.put(&mut l);
        l.buf.push('\n');
        sink::write(&l.buf);
    }
}

#[cfg(test)]
mod tests;
