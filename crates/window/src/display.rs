//! Displays and display modes. Pointers returned here are SDL's: arrays from [`displays`] and
//! [`fullscreen_modes`] are freed by the caller with [`crate::free`]; single modes are owned by SDL.

use crate::{Window, video};
use sdl3_sys::rect::{SDL_Point, SDL_Rect};
use sdl3_sys::video::*;
use std::ffi::{c_int, c_void};

pub use sdl3_sys::video::SDL_DisplayMode as DisplayMode;

/// SDL_GetDisplays: a 0-terminated `SDL_DisplayID` array (caller frees) and its length.
pub fn displays(count: &mut i32) -> *mut c_void {
    *count = 0;
    if !video::ensure() {
        return std::ptr::null_mut();
    }
    // SAFETY: count is a valid out pointer.
    unsafe { SDL_GetDisplays(count) }.cast()
}

pub fn primary() -> u32 {
    // SAFETY: plain query.
    if video::ensure() { unsafe { SDL_GetPrimaryDisplay() }.0 } else { 0 }
}

pub fn for_point(point: (i32, i32)) -> u32 {
    let point = SDL_Point { x: point.0, y: point.1 };
    // SAFETY: point lives across the call.
    if video::ensure() { unsafe { SDL_GetDisplayForPoint(&point) }.0 } else { 0 }
}

pub fn for_window(window: Window) -> u32 {
    // SAFETY: SDL validates the window.
    unsafe { SDL_GetDisplayForWindow(window.0) }.0
}

pub fn content_scale(display: u32) -> f32 {
    // SAFETY: plain query; an unknown display returns 0 with an error set.
    if video::ensure() { unsafe { SDL_GetDisplayContentScale(SDL_DisplayID(display)) } } else { 0.0 }
}

/// SDL_GetDisplayBounds / SDL_GetDisplayUsableBounds as (x, y, w, h).
pub fn bounds(display: u32, usable: bool) -> Option<[i32; 4]> {
    if !video::ensure() {
        return None;
    }
    let mut r = SDL_Rect { x: 0, y: 0, w: 0, h: 0 };
    let id = SDL_DisplayID(display);
    // SAFETY: r is a valid out pointer.
    let ok = unsafe { if usable { SDL_GetDisplayUsableBounds(id, &mut r) } else { SDL_GetDisplayBounds(id, &mut r) } };
    ok.then_some([r.x, r.y, r.w, r.h])
}

/// SDL_GetDesktopDisplayMode (`desktop`) or SDL_GetCurrentDisplayMode; null if unknown.
pub fn mode(display: u32, desktop: bool) -> *mut c_void {
    if !video::ensure() {
        return std::ptr::null_mut();
    }
    let id = SDL_DisplayID(display);
    // SAFETY: plain queries; the mode stays owned by SDL.
    let mode = unsafe { if desktop { SDL_GetDesktopDisplayMode(id) } else { SDL_GetCurrentDisplayMode(id) } };
    mode.cast_mut().cast()
}

/// SDL_GetFullscreenDisplayModes: a null-terminated `SDL_DisplayMode*` array (caller frees).
pub fn fullscreen_modes(display: u32, count: &mut i32) -> *mut c_void {
    *count = 0;
    if !video::ensure() {
        return std::ptr::null_mut();
    }
    // SAFETY: count is a valid out pointer.
    unsafe { SDL_GetFullscreenDisplayModes(SDL_DisplayID(display), count as *mut c_int) }.cast()
}

pub fn closest_fullscreen_mode(display: u32, w: i32, h: i32, refresh: f32, high_density: bool, out: &mut DisplayMode) -> bool {
    // SAFETY: out is a valid, writable SDL_DisplayMode.
    video::ensure() && unsafe { SDL_GetClosestFullscreenDisplayMode(SDL_DisplayID(display), w, h, refresh, high_density, out) }
}

/// SDL_SetWindowFullscreenMode. A null `mode` selects borderless desktop fullscreen.
///
/// # Safety
/// `mode` must be null or point to a readable `SDL_DisplayMode`.
pub unsafe fn set_window_fullscreen_mode(window: Window, mode: *const c_void) -> bool {
    unsafe { SDL_SetWindowFullscreenMode(window.0, mode.cast()) }
}
