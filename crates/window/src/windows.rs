//! Window creation and the per-window SDL calls. Every function takes the `SDL_Window*` managed
//! holds; SDL validates it (CHECK_WINDOW_MAGIC), so a stale or garbage handle fails with an error.

use crate::{Window, owned, props, video};
use sdl3_sys::video::*;
use std::ffi::{CStr, c_void};

/// Gotcha: SDL_WINDOW_VULKAN makes SDL load the Vulkan loader at creation and fail without one.
/// We present through wgpu from the raw handle, so the flag is dropped (see crate::handle).
const STRIPPED: u64 = SDL_WINDOW_VULKAN.0;

pub fn create(title: &CStr, w: i32, h: i32, flags: u64) -> Window {
    if !video::ensure() {
        return Window::NULL;
    }
    // SAFETY: title is NUL-terminated; SDL copies it.
    Window(unsafe { SDL_CreateWindow(title.as_ptr(), w, h, SDL_WindowFlags(flags & !STRIPPED)) })
}

pub fn create_with_properties(props: u32) -> Window {
    if !video::ensure() {
        return Window::NULL;
    }
    let name = c"SDL.window.create.flags";
    props::set_number(props, name, props::get_number(props, name, 0) & !(STRIPPED as i64));
    // SAFETY: plain SDL call; unknown property ids fail with an error.
    Window(unsafe { SDL_CreateWindowWithProperties(sdl3_sys::properties::SDL_PropertiesID(props)) })
}

pub fn create_popup(parent: Window, x: i32, y: i32, w: i32, h: i32, flags: u64) -> Window {
    // SAFETY: SDL validates the parent.
    Window(unsafe { SDL_CreatePopupWindow(parent.0, x, y, w, h, SDL_WindowFlags(flags & !STRIPPED)) })
}

pub fn destroy(window: Window) {
    // SAFETY: SDL validates the window.
    unsafe { SDL_DestroyWindow(window.0) }
}

pub fn set_title(window: Window, title: &CStr) -> bool {
    // SAFETY: title is NUL-terminated; SDL copies it.
    unsafe { SDL_SetWindowTitle(window.0, title.as_ptr()) }
}

pub fn title(window: Window) -> Option<String> {
    // SAFETY: SDL validates the window.
    owned(unsafe { SDL_GetWindowTitle(window.0) })
}

pub fn flags(window: Window) -> u64 {
    // SAFETY: SDL validates the window.
    unsafe { SDL_GetWindowFlags(window.0) }.0
}

pub fn from_id(id: u32) -> Window {
    // SAFETY: plain query.
    Window(unsafe { SDL_GetWindowFromID(SDL_WindowID(id)) })
}

pub fn id(window: Window) -> u32 {
    // SAFETY: SDL validates the window.
    unsafe { SDL_GetWindowID(window.0) }.0
}

/// SDL_SetWindowHitTest.
///
/// # Safety
/// `callback` must be null or an `SDL_HitTest` that stays callable while installed.
pub unsafe fn set_hit_test(window: Window, callback: *mut c_void, userdata: *mut c_void) -> bool {
    // SAFETY: a nullable C function pointer has the layout of Option<extern fn>.
    let callback: SDL_HitTest = unsafe { std::mem::transmute::<*mut c_void, SDL_HitTest>(callback) };
    unsafe { SDL_SetWindowHitTest(window.0, callback, userdata) }
}

/// (x, y) or (w, h) out of an SDL getter, or `None` when SDL fails.
fn pair(f: unsafe extern "C" fn(*mut SDL_Window, *mut i32, *mut i32) -> bool, window: Window) -> Option<(i32, i32)> {
    let (mut a, mut b) = (0, 0);
    // SAFETY: SDL validates the window; both outs are valid.
    unsafe { f(window.0, &mut a, &mut b) }.then_some((a, b))
}

pub fn position(window: Window) -> Option<(i32, i32)> {
    pair(SDL_GetWindowPosition, window)
}

pub fn size(window: Window) -> Option<(i32, i32)> {
    pair(SDL_GetWindowSize, window)
}

pub fn size_in_pixels(window: Window) -> Option<(i32, i32)> {
    pair(SDL_GetWindowSizeInPixels, window)
}

macro_rules! forward {
    ($($name:ident => $sdl:ident($($arg:ident: $ty:ty => $conv:expr),*) -> $ret:ty;)*) => {$(
        pub fn $name(window: Window $(, $arg: $ty)*) -> $ret {
            // SAFETY: SDL validates the window; the other arguments are plain values.
            unsafe { $sdl(window.0 $(, $conv)*) }
        }
    )*};
}

forward! {
    show => SDL_ShowWindow() -> bool;
    hide => SDL_HideWindow() -> bool;
    raise => SDL_RaiseWindow() -> bool;
    sync => SDL_SyncWindow() -> bool;
    pixel_density => SDL_GetWindowPixelDensity() -> f32;
    display_scale => SDL_GetWindowDisplayScale() -> f32;
    set_minimum_size => SDL_SetWindowMinimumSize(w: i32 => w, h: i32 => h) -> bool;
    set_maximum_size => SDL_SetWindowMaximumSize(w: i32 => w, h: i32 => h) -> bool;
    set_size => SDL_SetWindowSize(w: i32 => w, h: i32 => h) -> bool;
    set_aspect_ratio => SDL_SetWindowAspectRatio(min: f32 => min, max: f32 => max) -> bool;
    set_parent => SDL_SetWindowParent(parent: Window => parent.0) -> bool;
    set_fullscreen => SDL_SetWindowFullscreen(fullscreen: bool => fullscreen) -> bool;
}

/// Window-manager requests. A driver without a window manager (SDL's offscreen driver) has no hook
/// for them and SDL answers "unsupported"; there they are accepted no-ops, as a WM may ignore them.
macro_rules! forward_wm {
    ($($name:ident => $sdl:ident($($arg:ident: $ty:ty => $conv:expr),*);)*) => {$(
        pub fn $name(window: Window $(, $arg: $ty)*) -> bool {
            // SAFETY: SDL validates the window; the other arguments are plain values.
            let ok = unsafe { $sdl(window.0 $(, $conv)*) };
            ok || no_window_manager()
        }
    )*};
}

pub(crate) fn no_window_manager() -> bool {
    video::current_driver().as_deref() == Some(video::FALLBACK_DRIVER.to_str().unwrap_or_default())
}

forward_wm! {
    set_position => SDL_SetWindowPosition(x: i32 => x, y: i32 => y);
    minimize => SDL_MinimizeWindow();
    maximize => SDL_MaximizeWindow();
    restore => SDL_RestoreWindow();
    set_always_on_top => SDL_SetWindowAlwaysOnTop(on: bool => on);
    set_opacity => SDL_SetWindowOpacity(opacity: f32 => opacity);
    set_modal => SDL_SetWindowModal(modal: bool => modal);
    flash => SDL_FlashWindow(op: i32 => SDL_FlashOperation(op));
    set_bordered => SDL_SetWindowBordered(bordered: bool => bordered);
    set_resizable => SDL_SetWindowResizable(resizable: bool => resizable);
}
