//! Calling the function pointers managed registers on animatable objects. Call these with no
//! lock held: managed's handlers call straight back into our slots.

use std::ffi::c_void;

/// `AnimationControllerCreatedCallback`: `void ()`.
pub fn controller_created(f: usize) {
    if f != 0 {
        // SAFETY: managed passes a DelegateFunctionPointer for this exact signature and keeps it
        // alive for the SceneModel's life (SceneModel.Animation.cs); StdCall is the C ABI on x64.
        let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(f) };
        unsafe { f() }
    }
}

/// `LegacyAnimGraphChangedCallback`: `void (IntPtr)`; managed ignores the argument.
pub fn graph1_changed(f: usize, object: *mut c_void) {
    if f != 0 {
        // SAFETY: as above (SceneModel.LegacyAnimation.cs).
        let f: unsafe extern "C" fn(*mut c_void) = unsafe { std::mem::transmute(f) };
        unsafe { f(object) }
    }
}
