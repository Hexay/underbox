//! Facepunch's SDL extensions (`sbox/sdl_extensions.h`): desktop-integration calls SDL lacks.
//! Most are Windows shell features (taskbar, DWM corners and shadow, OLE drag and drop). Where
//! SDL has an equivalent it is used; the rest are accepted no-ops so panel windows still work.

use crate::{Window, set_error, windows};
use sdl3_sys::hints::{SDL_HINT_WINDOW_ACTIVATE_WHEN_RAISED, SDL_GetHint, SDL_SetHint};
use std::ffi::{CStr, c_void};

/// `CreateStartupWindow`: the splash window, later promoted to the game window. Native creates
/// it without SDL_WINDOW_VULKAN so it can hold a software framebuffer first; ours never sets it.
pub fn create_startup_window(title: &CStr, w: i32, h: i32, flags: u64) -> Window {
    windows::create(title, w, h, flags)
}

/// Raise above other windows without taking keyboard focus.
pub fn raise_without_focus(window: Window) {
    // SAFETY: the hint value is restored to SDL's copy (or cleared) after the raise.
    unsafe {
        let previous = SDL_GetHint(SDL_HINT_WINDOW_ACTIVATE_WHEN_RAISED);
        let previous = (!previous.is_null()).then(|| CStr::from_ptr(previous).to_owned());
        SDL_SetHint(SDL_HINT_WINDOW_ACTIVATE_WHEN_RAISED, c"0".as_ptr());
        windows::raise(window);
        SDL_SetHint(SDL_HINT_WINDOW_ACTIVATE_WHEN_RAISED, previous.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()));
    }
}

/// `SetWindowExternalDropTarget`: OLE drop callbacks for drags from other apps.
/// TODO: Windows IDropTarget; elsewhere SDL's own drop events (DropFile/DropText) still arrive.
pub fn set_external_drop_target(_window: Window, callbacks: *const c_void) -> bool {
    callbacks.is_null() || set_error("external drop targets are not supported on this platform yet")
}

/// `BeginExternalDrag`: start an OS drag of files/text; returns the DropAction (0 = none).
/// TODO: Windows DoDragDrop. Reporting "none" makes managed treat the drag as cancelled.
pub fn begin_external_drag(_window: Window, _files: &CStr, _text: &CStr) -> i32 {
    set_error("external drags are not supported on this platform yet");
    0
}

/// Window-chrome extensions with no SDL equivalent. TODO: Windows (DWM attributes, extended
/// window styles, ITaskbarList3) through the HWND from [`crate::handle`].
pub fn set_mouse_transparent(_window: Window, _transparent: bool) {}
pub fn set_can_maximize(_window: Window, _can_maximize: bool) {}
pub fn set_has_shadow(_window: Window, _has_shadow: bool) {}
pub fn set_rounded_corners(_window: Window, _rounded: bool) {}
pub fn send_to_back(_window: Window) {}
pub fn set_in_taskbar(_window: Window, _in_taskbar: bool) {}
pub fn set_can_close(_window: Window, _can_close: bool) {}
pub fn set_taskbar_overlay_icon(_window: Window, _w: i32, _h: i32, _pixels: *const c_void, _description: &CStr) {}
