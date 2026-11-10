//! Software surfaces: cursor and icon images, and the window framebuffer the startup splash draws
//! into before a swap chain exists (`FillWindowColor`, `PresentWindowPixels`).

use crate::Window;
use sdl3_sys::pixels::{SDL_PIXELFORMAT_RGBA32, SDL_PixelFormat};
use sdl3_sys::surface::*;
use sdl3_sys::video::{SDL_DestroyWindowSurface, SDL_GetWindowSurface, SDL_UpdateWindowSurface, SDL_WindowHasSurface};
use std::ffi::c_void;

/// An `SDL_Surface*`. Unlike windows SDL dereferences surfaces unchecked, hence `unsafe` creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surface(pub(crate) *mut SDL_Surface);

impl Surface {
    pub const NULL: Self = Self(std::ptr::null_mut());

    /// # Safety
    /// `ptr` must be null or a live surface from [`create_from`] (or SDL).
    pub unsafe fn from_ptr(ptr: *mut c_void) -> Self {
        Self(ptr.cast())
    }

    pub fn as_ptr(self) -> *mut c_void {
        self.0.cast()
    }
}

/// SDL_CreateSurfaceFrom: wraps caller pixels without copying.
///
/// # Safety
/// `pixels` must hold `height * pitch` bytes and outlive the surface.
pub unsafe fn create_from(width: i32, height: i32, format: u32, pixels: *mut c_void, pitch: i32) -> Surface {
    Surface(unsafe { SDL_CreateSurfaceFrom(width, height, SDL_PixelFormat(format as i32), pixels, pitch) })
}

/// # Safety
/// `surface` must be null or live, and not used afterwards.
pub unsafe fn destroy(surface: Surface) {
    unsafe { SDL_DestroySurface(surface.0) }
}

/// # Safety
/// `surface` must be null or live.
pub unsafe fn set_window_icon(window: Window, surface: Surface) -> bool {
    unsafe { sdl3_sys::video::SDL_SetWindowIcon(window.0, surface.0) }
}

pub fn window_has_surface(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_WindowHasSurface(window.0) }
}

pub fn destroy_window_surface(window: Window) -> bool {
    // SAFETY: SDL validates the window.
    unsafe { SDL_DestroyWindowSurface(window.0) }
}

/// Fill the window's software framebuffer with one colour and show it.
pub fn fill_window(window: Window, rgba: [u8; 4]) -> bool {
    // SAFETY: SDL validates the window; the surface it returns is live until the window resizes.
    unsafe {
        let target = SDL_GetWindowSurface(window.0);
        if target.is_null() {
            return false;
        }
        let [r, g, b, a] = rgba;
        SDL_FillSurfaceRect(target, std::ptr::null(), SDL_MapSurfaceRGBA(target, r, g, b, a)) && SDL_UpdateWindowSurface(window.0)
    }
}

/// Copy RGBA8 pixels into the window's framebuffer (scaled if the sizes differ) and show it.
///
/// # Safety
/// `pixels` must hold `height * pitch` readable bytes.
pub unsafe fn present_pixels(window: Window, width: i32, height: i32, pixels: *mut c_void, pitch: i32) -> bool {
    if pixels.is_null() || width <= 0 || height <= 0 {
        return crate::set_error("PresentWindowPixels: no pixels");
    }
    unsafe {
        let target = SDL_GetWindowSurface(window.0);
        if target.is_null() {
            return false;
        }
        let source = SDL_CreateSurfaceFrom(width, height, SDL_PIXELFORMAT_RGBA32, pixels, pitch);
        if source.is_null() {
            return false;
        }
        let blitted = if (*target).w == width && (*target).h == height {
            SDL_BlitSurface(source, std::ptr::null(), target, std::ptr::null())
        } else {
            SDL_BlitSurfaceScaled(source, std::ptr::null(), target, std::ptr::null(), SDL_SCALEMODE_LINEAR)
        };
        SDL_DestroySurface(source);
        blitted && SDL_UpdateWindowSurface(window.0)
    }
}

/// Read back the window framebuffer as tightly packed RGBA8 (tests; offscreen windows).
pub fn read_window(window: Window) -> Option<(i32, i32, Vec<u8>)> {
    // SAFETY: SDL validates the window; the converted copy is ours until destroyed.
    unsafe {
        let target = SDL_GetWindowSurface(window.0);
        if target.is_null() {
            return None;
        }
        let copy = SDL_ConvertSurface(target, SDL_PIXELFORMAT_RGBA32);
        if copy.is_null() {
            return None;
        }
        let (w, h, pitch) = ((*copy).w, (*copy).h, (*copy).pitch as usize);
        let rows = std::slice::from_raw_parts((*copy).pixels.cast::<u8>(), pitch * h as usize);
        let packed = rows.chunks(pitch).flat_map(|row| &row[..w as usize * 4]).copied().collect();
        SDL_DestroySurface(copy);
        Some((w, h, packed))
    }
}
