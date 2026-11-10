//! Windows without a display: video falls back to SDL's offscreen driver, where windows, their
//! software framebuffer, properties and window events all work. One test, since SDL video is
//! process-global. Where a real display exists (Windows CI) the same checks run against it.

use sdl3_sys::events::SDL_EVENT_WINDOW_RESIZED;
use window::{Window, clipboard, display, events, handle, props, surface, video, windows};

const HIDDEN: u64 = 0x8;
const RESIZABLE: u64 = 0x20;
const VULKAN: u64 = 0x1000_0000;

fn headless_linux() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

#[test]
fn windows_work_on_whatever_video_driver_comes_up() {
    assert!(video::ensure(), "{}", window::error());
    let driver = video::current_driver().expect("a driver");
    if headless_linux() {
        assert_eq!(driver, "offscreen");
    }

    let primary = display::primary();
    assert_ne!(primary, 0, "{}", window::error());
    let [_, _, w, h] = display::bounds(primary, false).expect("bounds");
    assert!(w > 0 && h > 0);
    let mut count = 0;
    let ids = display::displays(&mut count);
    assert!(count >= 1 && !ids.is_null());
    // SAFETY: SDL allocated the array; managed frees it the same way.
    unsafe { window::free(ids) };
    assert!(!display::mode(primary, true).is_null());

    // Managed's game window flags include Vulkan; creation must not depend on a Vulkan loader.
    let win = windows::create(c"underbox test", 64, 48, HIDDEN | RESIZABLE | VULKAN);
    assert!(!win.is_null(), "{}", window::error());
    assert_eq!(windows::size(win), Some((64, 48)));
    assert_eq!(windows::flags(win) & (HIDDEN | RESIZABLE), HIDDEN | RESIZABLE);
    assert_eq!(windows::flags(win) & VULKAN, 0);
    assert!(windows::set_title(win, c"renamed"));
    assert_eq!(windows::title(win).as_deref(), Some("renamed"));
    assert_eq!(windows::from_id(windows::id(win)), win);
    assert_ne!(props::of_window(win), 0);
    assert_eq!(display::for_window(win), primary);

    // The startup splash path: fill, then present RGBA pixels, then read the framebuffer back.
    assert!(surface::fill_window(win, [27, 36, 51, 255]), "{}", window::error());
    assert!(surface::window_has_surface(win));
    let (sw, sh, pixels) = surface::read_window(win).expect("framebuffer");
    assert_eq!((sw, sh), (64, 48));
    assert_eq!(&pixels[..4], &[27, 36, 51, 255]);
    let mut frame: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64) as u8, (i / 64) as u8, 200, 255]).collect();
    // SAFETY: frame holds 48 rows of 64*4 bytes.
    assert!(unsafe { surface::present_pixels(win, 64, 48, frame.as_mut_ptr().cast(), 64 * 4) }, "{}", window::error());
    let (_, _, pixels) = surface::read_window(win).expect("framebuffer");
    assert_eq!(pixels, frame);
    assert!(surface::destroy_window_surface(win));
    assert!(!surface::window_has_surface(win));

    match driver.as_str() {
        "offscreen" | "dummy" => assert!(handle::native(win).is_none()),
        _ => assert!(handle::native(win).is_some(), "{driver} should expose a native handle"),
    }

    // Window events carry the id; SDL resolves it back to the window managed holds.
    let mut resized = events::new(SDL_EVENT_WINDOW_RESIZED.0);
    resized.window.windowID = sdl3_sys::video::SDL_WindowID(windows::id(win));
    // SAFETY: a full SDL_Event.
    assert_eq!(unsafe { events::window_of(std::ptr::from_ref(&resized).cast()) }, win);

    assert!(clipboard::set_text(c"from underbox"));
    let text = clipboard::text();
    // SAFETY: SDL-allocated, NUL-terminated; freed once.
    assert_eq!(unsafe { std::ffi::CStr::from_ptr(text) }, c"from underbox");
    unsafe { window::free(text.cast()) };

    windows::destroy(win);
    assert!(!windows::show(win), "a destroyed window is rejected, not dereferenced");
    assert!(window::error().contains("Invalid window"), "{}", window::error());
    assert!(!windows::show(Window::from_ptr(0x1234 as *mut _)), "garbage handles are rejected too");
}
