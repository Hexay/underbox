//! Windows, events and input for underbox: SDL3 itself, built from source and linked statically.
//!
//! Managed s&box calls SDL3's API one function per `globalSdl` slot and reads SDL's own structs
//! (events, display modes), scancodes, keycodes and gamepad ids, so this crate exposes SDL with as
//! little in between as possible. Why SDL rather than winit: docs/renderer.md, "Windows and input".
//!
//! Handles are SDL's own pointers. SDL validates window, gamepad and cursor pointers against its
//! object registry before touching them, so those wrappers are safe to build from any value;
//! surfaces, events, callbacks and buffers are not validated and stay `unsafe` at the boundary.

pub mod clipboard;
pub mod dialog;
pub mod display;
pub mod events;
pub mod ext;
pub mod gamepad;
pub mod handle;
pub mod keyboard;
pub mod mouse;
pub mod props;
pub mod relative;
pub mod surface;
pub mod video;
pub mod windows;

use std::ffi::{CStr, c_char, c_void};

pub use sdl3_sys::events::SDL_Event as Event;

macro_rules! sdl_handle {
    ($(#[$m:meta])* $name:ident => $sdl:ty) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(*mut $sdl);

        impl $name {
            pub const NULL: Self = Self(std::ptr::null_mut());

            pub fn from_ptr(ptr: *mut c_void) -> Self {
                Self(ptr.cast())
            }

            pub fn as_ptr(self) -> *mut c_void {
                self.0.cast()
            }

            pub fn is_null(self) -> bool {
                self.0.is_null()
            }
        }
    };
}

sdl_handle!(
    /// An `SDL_Window*`: the handle managed keeps, compares and passes back.
    Window => sdl3_sys::video::SDL_Window
);
sdl_handle!(
    /// An `SDL_Gamepad*`.
    Gamepad => sdl3_sys::gamepad::SDL_Gamepad
);
sdl_handle!(
    /// An `SDL_Cursor*`.
    Cursor => sdl3_sys::mouse::SDL_Cursor
);

/// SDL's last error on this thread ("" when none).
pub fn error() -> String {
    // SAFETY: SDL_GetError never returns null; the string is valid until the next SDL call.
    unsafe { CStr::from_ptr(sdl3_sys::error::SDL_GetError()) }.to_string_lossy().into_owned()
}

/// Set SDL's error string (what managed reads back through `GetError`). Always returns false,
/// matching SDL's own convention so callers can `return set_error(..)`.
pub fn set_error(message: &str) -> bool {
    let message = std::ffi::CString::new(message.replace('\0', " ")).expect("NULs replaced");
    // SAFETY: a "%s" format with one C string argument.
    unsafe { sdl3_sys::error::SDL_SetError(c"%s".as_ptr(), message.as_ptr()) }
}

/// SDL_DelayPrecise: sleeps in 1 ms steps, then sleeps the rest less the worst overshoot seen and
/// spins only what is left.
pub fn delay_precise(ns: u64) {
    // SAFETY: plain value argument; SDL's tick source initialises itself on first use.
    unsafe { sdl3_sys::timer::SDL_DelayPrecise(ns) }
}

/// SDL_free: releases memory SDL handed out (clipboard text, display and mode arrays).
///
/// # Safety
/// `mem` must be null or a pointer SDL allocated and not yet freed.
pub unsafe fn free(mem: *mut c_void) {
    unsafe { sdl3_sys::stdinc::SDL_free(mem) }
}

/// An owned copy of a C string SDL returned (null becomes `None`).
fn owned(s: *const c_char) -> Option<String> {
    // SAFETY: SDL returns null or a NUL-terminated string valid until its next call.
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}
