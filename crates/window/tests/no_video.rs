//! A process whose gate refuses video (a dedicated server): nothing may open a display, every
//! call degrades instead of failing hard, and the clipboard still works in-process.
//! Its own test binary, so no other test has brought SDL up first.

use window::{Window, clipboard, display, events, video, windows};

#[test]
fn refused_video_degrades_without_opening_a_display() {
    video::set_gate(|| false);

    let mut event = events::new(0);
    assert!(!video::any_up());
    assert!(!events::poll(&mut event), "an uninitialised SDL has no events");

    assert_eq!(windows::create(c"server", 64, 64, 0), Window::NULL);
    assert!(window::error().contains("dedicated server"), "{}", window::error());
    assert_eq!(display::primary(), 0);
    assert_eq!(display::bounds(1, false), None);
    let mut count = -1;
    assert!(display::displays(&mut count).is_null());
    assert_eq!(count, 0);
    assert!(window::mouse::system_cursor(0).is_null());
    assert!(!video::is_up(), "nothing above may bring video up");
    assert_eq!(video::current_driver(), None);

    assert!(!clipboard::has_text());
    let empty = clipboard::text();
    // SAFETY: SDL-allocated, NUL-terminated; freed once below.
    assert_eq!(unsafe { std::ffi::CStr::from_ptr(empty) }, c"");
    assert!(clipboard::set_text(c"copied on a server"));
    assert!(clipboard::has_text());
    let text = clipboard::text();
    // SAFETY: as above.
    assert_eq!(unsafe { std::ffi::CStr::from_ptr(text) }, c"copied on a server");
    // SAFETY: both came from clipboard::text, which allocates with SDL's allocator.
    unsafe {
        window::free(empty.cast());
        window::free(text.cast());
    }
    assert!(!video::is_up());
}
