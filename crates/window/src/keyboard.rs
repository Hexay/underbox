//! Keyboard state, key names and text input. Scancodes and keycodes are SDL's own values:
//! managed maps them in `KeyTranslation.cs`. Without a keyboard driver SDL uses its default
//! US layout, so name and keycode lookups work headless.

use crate::{Window, owned};
use sdl3_sys::keyboard::*;
use sdl3_sys::keycode::{SDL_Keycode, SDL_Keymod};
use sdl3_sys::rect::SDL_Rect;
use sdl3_sys::scancode::SDL_Scancode;

/// SDL_GetKeyFromScancode. Managed passes `SDL_Keymod` widened to 32 bits; only 16 are defined.
pub fn key_from_scancode(scancode: i32, modifiers: u32, key_event: bool) -> u32 {
    // SAFETY: plain lookup; out-of-range scancodes return SDLK_UNKNOWN.
    unsafe { SDL_GetKeyFromScancode(SDL_Scancode(scancode), SDL_Keymod(modifiers as u16), key_event) }.0
}

pub fn scancode_from_key(key: u32) -> i32 {
    // SAFETY: plain lookup; a null modstate out pointer is allowed.
    unsafe { SDL_GetScancodeFromKey(SDL_Keycode(key), std::ptr::null_mut()) }.0
}

/// SDL_GetKeyName ("" for an unknown key).
pub fn key_name(key: u32) -> String {
    // SAFETY: plain lookup.
    owned(unsafe { SDL_GetKeyName(SDL_Keycode(key)) }).unwrap_or_default()
}

pub fn scancode_name(scancode: i32) -> String {
    // SAFETY: plain lookup.
    owned(unsafe { SDL_GetScancodeName(SDL_Scancode(scancode)) }).unwrap_or_default()
}

pub fn mod_state() -> u32 {
    // SAFETY: plain query.
    u32::from(unsafe { SDL_GetModState() }.0)
}

pub fn set_mod_state(modifiers: u32) {
    // SAFETY: plain SDL call.
    unsafe { SDL_SetModState(SDL_Keymod(modifiers as u16)) }
}

pub fn focus() -> Window {
    // SAFETY: plain query.
    Window(unsafe { SDL_GetKeyboardFocus() })
}

pub fn start_text_input(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_StartTextInput(window.0) }
}

pub fn stop_text_input(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_StopTextInput(window.0) }
}

pub fn text_input_active(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_TextInputActive(window.0) }
}

pub fn clear_composition(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_ClearComposition(window.0) }
}

/// SDL_SetTextInputArea: where the IME candidate window goes, as (x, y, w, h) in window points.
pub fn set_text_input_area(window: Window, area: [i32; 4], cursor: i32) -> bool {
    let rect = SDL_Rect { x: area[0], y: area[1], w: area[2], h: area[3] };
    // SAFETY: SDL validates the window and copies the rect.
    unsafe { SDL_SetTextInputArea(window.0, &rect, cursor) }
}
