//! Mouse state, capture, relative mode and cursors.

use crate::surface::Surface;
use crate::{Cursor, Window, video};
use sdl3_sys::mouse::*;

/// SDL_GetMouseState (window-relative) or SDL_GetGlobalMouseState: button mask and position.
pub fn state(global: bool) -> (u32, f32, f32) {
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: both outs are valid.
    let buttons = unsafe { if global { SDL_GetGlobalMouseState(&mut x, &mut y) } else { SDL_GetMouseState(&mut x, &mut y) } };
    (buttons.0, x, y)
}

pub fn capture(enabled: bool) -> bool {
    // SAFETY: plain SDL call; fails with an error without video.
    unsafe { SDL_CaptureMouse(enabled) }
}

pub fn focus() -> Window {
    // SAFETY: plain query.
    Window(unsafe { SDL_GetMouseFocus() })
}

pub fn set_relative_mode(window: Window, relative: bool) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_SetWindowRelativeMouseMode(window.0, relative) }
}

pub fn relative_mode(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_GetWindowRelativeMouseMode(window.0) }
}

pub fn warp(window: Window, x: f32, y: f32) {
    // SAFETY: SDL validates the window (null means the focused one).
    unsafe { SDL_WarpMouseInWindow(window.0, x, y) }
}

/// SDL_CreateSystemCursor; `kind` is an `SDL_SystemCursor`.
pub fn system_cursor(kind: i32) -> Cursor {
    if !video::ensure() {
        return Cursor::NULL;
    }
    // SAFETY: plain SDL call; an unknown kind fails with an error.
    Cursor(unsafe { SDL_CreateSystemCursor(SDL_SystemCursor(kind)) })
}

/// SDL_CreateColorCursor from a surface (SDL copies the pixels).
pub fn color_cursor(surface: Surface, hot_x: i32, hot_y: i32) -> Cursor {
    if !video::ensure() {
        return Cursor::NULL;
    }
    // SAFETY: a Surface is null or a live SDL surface (its constructor's contract).
    Cursor(unsafe { SDL_CreateColorCursor(surface.0, hot_x, hot_y) })
}

pub fn destroy_cursor(cursor: Cursor) {
    // SAFETY: SDL looks the cursor up in its list before freeing; unknown pointers are ignored.
    unsafe { SDL_DestroyCursor(cursor.0) }
}

/// SDL_SetCursor; null redraws the current cursor.
pub fn set_cursor(cursor: Cursor) -> bool {
    // SAFETY: SDL looks the cursor up in its list and fails on unknown pointers.
    unsafe { SDL_SetCursor(cursor.0) }
}

pub fn show_cursor() -> bool {
    // SAFETY: plain SDL call.
    unsafe { SDL_ShowCursor() }
}

pub fn hide_cursor() -> bool {
    // SAFETY: plain SDL call.
    unsafe { SDL_HideCursor() }
}
