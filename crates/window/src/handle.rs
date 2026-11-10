//! Native handles for the renderer: an SDL window as a raw-window-handle pair.
//!
//! The renderer creates its `wgpu::Surface` from these:
//! ```ignore
//! let h = window::handle::native(win).expect("window has a native surface");
//! // SAFETY: the SDL window outlives the surface (managed destroys the swap chain first).
//! let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
//!     raw_display_handle: h.display, raw_window_handle: h.window }) }?;
//! ```
//! `None` means the window has no native surface: SDL's offscreen/dummy drivers (headless) or a
//! platform not wired yet (macOS: TODO, needs an NSView from SDL_Metal_CreateView). The renderer
//! then renders to an offscreen target instead.

use crate::{Window, video};
use raw_window_handle::*;
use sdl3_sys::properties::{SDL_GetNumberProperty, SDL_GetPointerProperty, SDL_PropertiesID};
use sdl3_sys::video::*;
use std::ffi::{c_char, c_void};
use std::ptr::NonNull;

/// A window's raw handles. Valid while the SDL window lives; `Copy` so the renderer can keep it.
#[derive(Clone, Copy, Debug)]
pub struct NativeHandles {
    pub window: RawWindowHandle,
    pub display: RawDisplayHandle,
}

fn pointer(props: SDL_PropertiesID, name: *const c_char) -> Option<NonNull<c_void>> {
    // SAFETY: name is one of SDL's NUL-terminated property-name constants.
    NonNull::new(unsafe { SDL_GetPointerProperty(props, name, std::ptr::null_mut()) })
}

fn number(props: SDL_PropertiesID, name: *const c_char) -> i64 {
    // SAFETY: as above.
    unsafe { SDL_GetNumberProperty(props, name, 0) }
}

/// The raw handles of `window`, or `None` when it has no native surface (see module docs).
pub fn native(window: Window) -> Option<NativeHandles> {
    if window.is_null() || !video::is_up() {
        return None;
    }
    // SAFETY: SDL validates the window; an invalid one yields property id 0, which reads as empty.
    let props = unsafe { SDL_GetWindowProperties(window.0) };
    match video::current_driver()?.as_str() {
        "x11" => {
            let display = pointer(props, SDL_PROP_WINDOW_X11_DISPLAY_POINTER)?;
            let screen = number(props, SDL_PROP_WINDOW_X11_SCREEN_NUMBER) as i32;
            let xid = number(props, SDL_PROP_WINDOW_X11_WINDOW_NUMBER);
            (xid != 0).then(|| NativeHandles {
                window: XlibWindowHandle::new(xid as _).into(),
                display: XlibDisplayHandle::new(Some(display), screen).into(),
            })
        }
        "wayland" => Some(NativeHandles {
            window: WaylandWindowHandle::new(pointer(props, SDL_PROP_WINDOW_WAYLAND_SURFACE_POINTER)?).into(),
            display: WaylandDisplayHandle::new(pointer(props, SDL_PROP_WINDOW_WAYLAND_DISPLAY_POINTER)?).into(),
        }),
        "windows" => {
            let hwnd = std::num::NonZeroIsize::new(pointer(props, SDL_PROP_WINDOW_WIN32_HWND_POINTER)?.as_ptr() as isize)?;
            let mut handle = Win32WindowHandle::new(hwnd);
            handle.hinstance = pointer(props, SDL_PROP_WINDOW_WIN32_INSTANCE_POINTER)
                .and_then(|p| std::num::NonZeroIsize::new(p.as_ptr() as isize));
            Some(NativeHandles { window: handle.into(), display: WindowsDisplayHandle::new().into() })
        }
        _ => None,
    }
}
