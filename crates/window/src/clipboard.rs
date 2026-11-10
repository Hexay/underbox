//! The clipboard. SDL's needs video; without it (a server, or video refused) text is kept
//! in-process so copy and paste inside the app still work.

use crate::video;
use sdl3_sys::clipboard::*;
use std::ffi::{CStr, CString, c_char};
use std::sync::Mutex;

static LOCAL: Mutex<Option<CString>> = Mutex::new(None);

fn local() -> std::sync::MutexGuard<'static, Option<CString>> {
    LOCAL.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn has_text() -> bool {
    if video::ensure() {
        // SAFETY: plain query.
        return unsafe { SDL_HasClipboardText() };
    }
    local().as_ref().is_some_and(|t| !t.is_empty())
}

pub fn set_text(text: &CStr) -> bool {
    if video::ensure() {
        // SAFETY: text is NUL-terminated; SDL copies it.
        return unsafe { SDL_SetClipboardText(text.as_ptr()) };
    }
    *local() = Some(text.to_owned());
    true
}

/// SDL_GetClipboardText: an SDL-allocated string ("" when empty) the caller frees with
/// [`crate::free`], as managed does. The in-process copy is allocated the same way.
pub fn text() -> *mut c_char {
    if video::ensure() {
        // SAFETY: plain query; SDL allocates the result.
        return unsafe { SDL_GetClipboardText() };
    }
    let guard = local();
    let text = guard.as_deref().unwrap_or(c"");
    // SAFETY: text is NUL-terminated; SDL_strdup allocates with SDL's allocator.
    unsafe { sdl3_sys::stdinc::SDL_strdup(text.as_ptr()) }
}
