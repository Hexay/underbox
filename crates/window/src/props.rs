//! SDL property groups (`SDL_PropertiesID`): managed builds window-creation properties with them
//! and reads native handles (`SDL.window.win32.hwnd`) from a window's group.

use crate::Window;
use sdl3_sys::properties::*;
use std::ffi::{CStr, c_void};

pub fn create() -> u32 {
    // SAFETY: plain SDL call; works without any subsystem.
    unsafe { SDL_CreateProperties() }.0
}

pub fn destroy(props: u32) {
    // SAFETY: SDL ignores unknown ids.
    unsafe { SDL_DestroyProperties(SDL_PropertiesID(props)) }
}

pub fn of_window(window: Window) -> u32 {
    // SAFETY: SDL validates the window.
    unsafe { sdl3_sys::video::SDL_GetWindowProperties(window.0) }.0
}

#[expect(clippy::not_unsafe_ptr_arg_deref, reason = "SDL returns the default unread")]
pub fn get_pointer(props: u32, name: &CStr, default: *mut c_void) -> *mut c_void {
    // SAFETY: name is NUL-terminated; the default is returned as is, never dereferenced.
    unsafe { SDL_GetPointerProperty(SDL_PropertiesID(props), name.as_ptr(), default) }
}

pub fn get_number(props: u32, name: &CStr, default: i64) -> i64 {
    // SAFETY: name is NUL-terminated.
    unsafe { SDL_GetNumberProperty(SDL_PropertiesID(props), name.as_ptr(), default) }
}

pub fn set_string(props: u32, name: &CStr, value: &CStr) -> bool {
    // SAFETY: both NUL-terminated; SDL copies the value.
    unsafe { SDL_SetStringProperty(SDL_PropertiesID(props), name.as_ptr(), value.as_ptr()) }
}

pub fn set_number(props: u32, name: &CStr, value: i64) -> bool {
    // SAFETY: name is NUL-terminated.
    unsafe { SDL_SetNumberProperty(SDL_PropertiesID(props), name.as_ptr(), value) }
}

pub fn set_boolean(props: u32, name: &CStr, value: bool) -> bool {
    // SAFETY: name is NUL-terminated.
    unsafe { SDL_SetBooleanProperty(SDL_PropertiesID(props), name.as_ptr(), value) }
}

pub fn get_boolean(props: u32, name: &CStr, default: bool) -> bool {
    // SAFETY: name is NUL-terminated.
    unsafe { SDL_GetBooleanProperty(SDL_PropertiesID(props), name.as_ptr(), default) }
}
