//! Native file dialogs (SDL's: the portal or zenity on Linux, the shell on Windows). Results come
//! back asynchronously through managed's `SDL_DialogFileCallback`.

use crate::{Window, video};
use sdl3_sys::dialog::*;
use std::ffi::{c_char, c_void};

fn callback(f: *mut c_void) -> SDL_DialogFileCallback {
    // SAFETY: a nullable C function pointer has the layout of Option<extern fn>.
    unsafe { std::mem::transmute::<*mut c_void, SDL_DialogFileCallback>(f) }
}

/// Without video no dialog can open: report failure the way SDL does (null file list), so
/// managed's pending request completes instead of waiting forever.
unsafe fn fail(cb: *mut c_void, userdata: *mut c_void) {
    if let Some(cb) = callback(cb) {
        unsafe { cb(userdata, std::ptr::null(), -1) }
    }
}

/// SDL_ShowOpenFolderDialog (single selection: s&box's `ShowOpenFolderDialogAt`).
///
/// # Safety
/// `cb` must be an `SDL_DialogFileCallback` callable until it has run; `default_path` must be
/// null or NUL-terminated.
pub unsafe fn open_folder(cb: *mut c_void, userdata: *mut c_void, window: Window, default_path: *const c_char) {
    if !video::ensure() {
        return unsafe { fail(cb, userdata) };
    }
    unsafe { SDL_ShowOpenFolderDialog(callback(cb), userdata, window.0, default_path, false) }
}

/// SDL_ShowOpenFileDialog.
///
/// # Safety
/// As [`open_folder`]; `filters` must be null or point to `count` `SDL_DialogFileFilter`s that
/// stay valid until the callback has run.
pub unsafe fn open_file(
    cb: *mut c_void,
    userdata: *mut c_void,
    window: Window,
    filters: *const c_void,
    count: i32,
    default_path: *const c_char,
    allow_many: bool,
) {
    if !video::ensure() {
        return unsafe { fail(cb, userdata) };
    }
    unsafe { SDL_ShowOpenFileDialog(callback(cb), userdata, window.0, filters.cast(), count, default_path, allow_many) }
}

/// SDL_ShowSaveFileDialog.
///
/// # Safety
/// As [`open_file`].
pub unsafe fn save_file(cb: *mut c_void, userdata: *mut c_void, window: Window, filters: *const c_void, count: i32, default_path: *const c_char) {
    if !video::ensure() {
        return unsafe { fail(cb, userdata) };
    }
    unsafe { SDL_ShowSaveFileDialog(callback(cb), userdata, window.0, filters.cast(), count, default_path) }
}
