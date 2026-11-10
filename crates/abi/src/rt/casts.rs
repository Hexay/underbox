//! The cast slots (InteropGen's `dynamic_cast` thunks). Our objects never adjust pointers between
//! base and derived, so a cast is the same pointer, or null when the object isn't of the target class.

use crate::generated::slots::CLASS_BASES;
use crate::rt::types::NativeRef;
use std::sync::OnceLock;

/// A native class marker (`abi::class::*`).
pub trait NativeClass: 'static {
    const NAME: &'static str;
}

/// What class the object at an address was created as; installed by the object registry.
pub type ClassOf = fn(usize) -> Option<&'static str>;

static CLASS_OF: OnceLock<ClassOf> = OnceLock::new();

pub fn set_class_of(f: ClassOf) {
    let _ = CLASS_OF.set(f);
}

/// Whether `class` is `ancestor` or derives from it.
pub fn derives_from(class: &str, ancestor: &str) -> bool {
    let mut current = Some(class);
    while let Some(c) = current {
        if c == ancestor {
            return true;
        }
        current = CLASS_BASES.iter().find(|(d, _)| *d == c).map(|(_, b)| *b);
    }
    false
}

pub(crate) fn up<D: NativeClass, B: NativeClass>(r: NativeRef<D>) -> NativeRef<B> {
    r.cast()
}

pub(crate) fn down<B: NativeClass, D: NativeClass>(r: NativeRef<B>) -> NativeRef<D> {
    let class = CLASS_OF.get().and_then(|f| f(r.addr()));
    match class {
        Some(c) if derives_from(c, D::NAME) => r.cast(),
        _ => NativeRef::NULL,
    }
}
