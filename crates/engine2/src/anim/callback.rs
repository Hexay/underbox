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

/// `AnimEventCallback`: `void (char* typeName, float cycle, float time, KeyValues3* data)`, once
/// per event; the payload lives only for the call.
pub fn animation_events(f: usize, events: &[anim::clip::Fired]) {
    if f == 0 {
        return;
    }
    // SAFETY: managed passes a DelegateFunctionPointer for this signature (SceneModel.Events.cs)
    // and keeps it alive for the SceneModel's life.
    let f: unsafe extern "C" fn(*const std::ffi::c_char, f32, f32, *mut c_void) = unsafe { std::mem::transmute(f) };
    for e in events {
        let name = std::ffi::CString::new(e.name.replace('\0', "")).unwrap_or_default();
        let node = crate::kv3::node::from_value(&e.data);
        let data = crate::kv3::node::handle(&node);
        unsafe { f(name.as_ptr(), e.cycle, e.time, data.as_raw().cast()) }
        crate::kv3::node::release(&node);
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
