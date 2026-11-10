//! The managed functions native may call, as `igen_engine` hands them over.

use crate::generated::slots::CALLBACK_COUNT;
use std::sync::OnceLock;

static TABLE: OnceLock<Vec<usize>> = OnceLock::new();

/// Keep managed's callback table. Managed pins its array only for the `igen_engine` call, so it's copied.
///
/// # Safety
/// `table` points at `CALLBACK_COUNT` function pointers.
pub unsafe fn set_table(table: *const usize) {
    let copy = unsafe { std::slice::from_raw_parts(table, CALLBACK_COUNT) }.to_vec();
    let _ = TABLE.set(copy);
}

/// Whether managed handed over callback `index` (false in unit tests and before `igen_engine`).
pub fn is_set(index: usize) -> bool {
    TABLE.get().is_some_and(|t| t.get(index).is_some_and(|p| *p != 0))
}

/// Callback `index` as the signature `F`.
///
/// # Safety
/// `F` is the callback's exact `extern "C"` signature.
pub(crate) unsafe fn callback<F: Copy>(index: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let p = TABLE.get().map_or(0, |t| t[index]);
    assert!(p != 0, "managed callback {index} called before igen_engine handed the table over");
    unsafe { std::mem::transmute_copy::<usize, F>(&p) }
}
