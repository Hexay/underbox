//! Relative mouse mode where SDL has none: SDL's X11 backend needs XInput2, which an SDL built without
//! libxi-dev's headers lacks (build.rs), so `SDL_SetWindowRelativeMouseMode` fails "not supported" and a
//! game's mouse look never turns. Emulated as SDL2's warp mode did: the cursor is hidden, each motion
//! event's offset from the window centre becomes its `xrel/yrel`, and the pointer is warped back.

use crate::{Event, Window};
use sdl3_sys::events::SDL_EVENT_MOUSE_MOTION;
use sdl3_sys::mouse::{SDL_HideCursor, SDL_ShowCursor, SDL_WarpMouseInWindow};
use sdl3_sys::video::{SDL_GetWindowID, SDL_GetWindowSize, SDL_WindowID};
use std::sync::Mutex;

/// The emulating window (managed captures one at a time).
static ACTIVE: Mutex<Option<(usize, SDL_WindowID)>> = Mutex::new(None);

fn center(window: Window) -> (f32, f32) {
    let (mut w, mut h) = (0, 0);
    // SAFETY: SDL validates the window; both outs are valid.
    unsafe { SDL_GetWindowSize(window.0, &mut w, &mut h) };
    ((w / 2) as f32, (h / 2) as f32)
}

/// Turn emulation on or off for `window`. Always succeeds.
pub fn set(window: Window, on: bool) -> bool {
    let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    let was = active.is_some_and(|(w, _)| w == window.0 as usize);
    if on && !was {
        // SAFETY: SDL validates the window.
        let id = unsafe { SDL_GetWindowID(window.0) };
        *active = Some((window.0 as usize, id));
        let (x, y) = center(window);
        // SAFETY: plain SDL calls on a validated window.
        unsafe {
            SDL_HideCursor();
            SDL_WarpMouseInWindow(window.0, x, y);
        }
    } else if !on && was {
        *active = None;
        // SAFETY: plain SDL call.
        unsafe { SDL_ShowCursor() };
    }
    true
}

pub fn is_on(window: Window) -> bool {
    ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|(w, _)| w == window.0 as usize)
}

/// Rewrite a polled event for emulation; false drops it (the warp's own motion back to the centre).
pub fn filter(event: &mut Event) -> bool {
    // SAFETY: `type` is valid for every event.
    if unsafe { event.r#type } != SDL_EVENT_MOUSE_MOTION.0 {
        return true;
    }
    let Some((ptr, id)) = *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) else { return true };
    // SAFETY: a motion event's `motion` member is the active one.
    let motion = unsafe { &mut event.motion };
    if motion.windowID != id {
        return true;
    }
    let window = Window(ptr as *mut _);
    let (cx, cy) = center(window);
    let (dx, dy) = (motion.x - cx, motion.y - cy);
    if dx.abs() < 0.5 && dy.abs() < 0.5 {
        return false;
    }
    (motion.xrel, motion.yrel) = (dx, dy);
    // SAFETY: SDL validates the window.
    unsafe { SDL_WarpMouseInWindow(window.0, cx, cy) };
    true
}
