//! 2D physics: managed's `NativeEngine.Box2d` (the `globalBox2d` slots) forwarded to Box2D itself.
//!
//! `box2d.def` mirrors Box2D's C API one to one, so almost every slot is a forward: the abi structs
//! and Box2D's share their layout (`layout_tests.rs`), and [`cast`] reinterprets one as the other.
//! The `inline` wrappers in `box2d.def` take a def struct by value where C takes a pointer; [`fwd!`]
//! passes the arguments after its `;` by pointer for those.
//!
//! Managed callbacks (queries, pre-solve, custom filter, debug draw) are function pointers Box2D
//! calls during the forwarded call, as in real engine2. No Rust lock is held around any of them.

use box2d_sys as sys;

/// Reinterprets `a` as its layout-identical twin (abi type ⇄ Box2D type, or the same type).
pub(crate) fn cast<A: Copy, B: Copy>(a: A) -> B {
    const { assert!(size_of::<A>() == size_of::<B>(), "cast between types of different size") };
    // SAFETY: the sizes match (above) and every pair this module casts shares its layout, which
    // layout_tests.rs asserts field by field; ids and plain numbers have no invalid bit patterns.
    unsafe { std::mem::transmute_copy(&a) }
}

/// Binds each `SLOT => b2Function(args)` to a closure that forwards to Box2D. The closure's
/// signature comes from the slot, so a wrong arity or a size mismatch fails to compile.
macro_rules! fwd {
    ($r:ident: $($slot:ident => $f:ident($($a:ident),* $(; $($p:ident),+)?);)*) => {$(
        $r.bind(
            abi::slots::global_box2d::$slot,
            |$($a,)* $($($p),+)?| {
                // SAFETY: Box2D validates ids itself; pointers are managed's, as for real engine2.
                $crate::physics2d::cast(unsafe { box2d_sys::$f($($crate::physics2d::cast($a),)* $($(&$crate::physics2d::cast($p)),+)?) })
            },
            module_path!(),
        );
    )*};
}

mod body;
mod defaults;
mod joint;
mod joint_kinds;
mod shape;
mod world;

#[cfg(test)]
mod layout;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;

pub fn register(r: &mut abi::Registry) {
    world::register(r);
    body::register(r);
    shape::register(r);
    joint::register(r);
    joint_kinds::register(r);
    defaults::register(r);
}