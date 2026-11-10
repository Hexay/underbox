//! Helpers the generated thunks (`generated/thunks.rs`) are written in.

use crate::rt::registry::{Slot, implementation, stub_hit};

#[inline]
pub(crate) fn bound<F: Copy>(slot: Slot<F>) -> Option<F> {
    implementation::<F>(slot.index)
}

/// An unbound slot's answer: zero (null, false, 0.0, a zeroed struct).
/// `R` is only ever a thunk's ABI return type, all plain data for which all-zero is valid.
#[cold]
pub(crate) fn unbound<R>(index: usize) -> R {
    stub_hit(index);
    // SAFETY: see above; R is never a reference or a non-nullable type.
    unsafe { std::mem::zeroed() }
}

/// A struct managed passes by pointer to mean "by value".
///
/// # Safety
/// `p` is null or points at a readable `T`.
#[inline]
pub(crate) unsafe fn by_val<T: Copy + Default>(p: *const T) -> T {
    if p.is_null() { T::default() } else { unsafe { p.read_unaligned() } }
}

/// An `out`/`ref` argument. Managed always passes a local's address; a null still gets somewhere
/// to write rather than undefined behaviour.
///
/// # Safety
/// `p` is null or points at a `T` that's valid, aligned and unaliased for the call.
#[inline]
pub(crate) unsafe fn out_ref<T>(p: *mut T, scratch: &mut T) -> &mut T {
    if p.is_null() { scratch } else { unsafe { &mut *p } }
}

/// A buffer managed passed as pointer + element count; empty when null or the count is ≤ 0.
///
/// # Safety
/// `p` is null or points at `len` `T`s that are valid, aligned and unaliased for the call.
#[inline]
pub(crate) unsafe fn slice_arg<'a, T, P>(p: *mut P, len: impl TryInto<usize>) -> &'a mut [T] {
    match len.try_into() {
        Ok(n) if n > 0 && !p.is_null() => unsafe { std::slice::from_raw_parts_mut(p.cast::<T>(), n) },
        _ => &mut [],
    }
}
