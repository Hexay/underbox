//! Emulated relative mouse mode (window::relative): motion becomes an offset from the window centre,
//! and the warp back to the centre is dropped. Its own process: SDL video is process-global.

use sdl3_sys::events::SDL_EVENT_MOUSE_MOTION;
use sdl3_sys::video::SDL_GetWindowID;
use window::{events, relative, video, windows};

const HIDDEN: u64 = 0x8;

#[test]
fn motion_is_measured_from_the_centre_and_warps_are_dropped() {
    assert!(video::ensure(), "{}", window::error());
    let win = windows::create(c"relative", 200, 100, HIDDEN);
    assert!(!win.is_null(), "{}", window::error());
    assert!(relative::set(win, true));
    assert!(relative::is_on(win));
    let mut e = events::new(0);
    while events::poll(&mut e) {}

    // SAFETY: SDL validates the window.
    let id = unsafe { SDL_GetWindowID(win.as_ptr().cast()) };
    let motion = |x: f32, y: f32| {
        let mut e = events::new(SDL_EVENT_MOUSE_MOTION.0);
        e.motion.windowID = id;
        (e.motion.x, e.motion.y) = (x, y);
        e
    };
    events::push(&mut motion(100.0, 50.0));
    events::push(&mut motion(130.0, 46.0));
    let mut got = Vec::new();
    while events::poll(&mut e) {
        // SAFETY: `type` is valid for every event; `motion` for motion events.
        if unsafe { e.r#type } == SDL_EVENT_MOUSE_MOTION.0 && unsafe { e.motion.windowID } == id {
            got.push(unsafe { (e.motion.xrel, e.motion.yrel) });
        }
    }
    assert_eq!(got, [(30.0, -4.0)], "the centred event is the warp's own and is dropped");

    assert!(relative::set(win, false));
    assert!(!relative::is_on(win));
    windows::destroy(win);
}
