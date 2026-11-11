//! The SDL event queue. Managed's `NativeEngine.Sdl.Event` mirrors `SDL_Event` byte for byte
//! (128 bytes, `type` at 0; checked in tests/layout.rs), so events are written straight through.
//! Strings inside events (text input, drops) are SDL's temporary memory, valid until the next poll.

use crate::{Event, Window, video};
use sdl3_sys::events::*;
use std::ffi::c_void;

/// SDL_PollEvent. False without touching SDL when nothing is initialised (a server's loop).
pub fn poll(out: &mut Event) -> bool {
    if !video::any_up() {
        return false;
    }
    // SAFETY: out is a valid, writable SDL_Event.
    while unsafe { SDL_PollEvent(out) } {
        if crate::relative::filter(out) {
            return true;
        }
    }
    false
}

/// SDL_PushEvent (synthetic input for tests and tools). SDL stamps the timestamp.
pub fn push(event: &mut Event) -> bool {
    // SAFETY: event is a valid SDL_Event; SDL copies it.
    unsafe { SDL_PushEvent(event) }
}

/// A zeroed event of `kind` (an `SDL_EventType` value).
pub fn new(kind: u32) -> Event {
    // SAFETY: SDL_Event is a plain-data union; all-zero is valid.
    let mut e: Event = unsafe { std::mem::zeroed() };
    e.r#type = kind;
    e
}

fn filter(callback: *mut c_void) -> SDL_EventFilter {
    // SAFETY: a nullable C function pointer has the layout of Option<extern fn>.
    unsafe { std::mem::transmute::<*mut c_void, SDL_EventFilter>(callback) }
}

/// SDL_AddEventWatch: `callback(userdata, event)` runs for every event as it is queued.
///
/// # Safety
/// `callback` must be an `SDL_EventFilter` that stays callable until removed.
pub unsafe fn add_watch(callback: *mut c_void, userdata: *mut c_void) -> bool {
    unsafe { SDL_AddEventWatch(filter(callback), userdata) }
}

/// SDL_RemoveEventWatch.
///
/// # Safety
/// As for [`add_watch`]; SDL only compares the pointers.
pub unsafe fn remove_watch(callback: *mut c_void, userdata: *mut c_void) {
    unsafe { SDL_RemoveEventWatch(filter(callback), userdata) }
}

/// SDL_GetWindowFromEvent: the window a window/input event targets, or null.
///
/// # Safety
/// `event` must be null or point to a readable 128-byte SDL event.
pub unsafe fn window_of(event: *const c_void) -> Window {
    if event.is_null() {
        return Window::NULL;
    }
    Window(unsafe { SDL_GetWindowFromEvent(event.cast()) })
}
