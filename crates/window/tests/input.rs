//! Input without a display: SDL's key tables, events through managed's struct, gamepad mappings.

use abi::structs::NativeEngine_Sdl_Event as ManagedEvent;
use sdl3_sys::events::*;
use sdl3_sys::video::SDL_WindowID;
use std::sync::Once;
use window::{events, gamepad, keyboard, video};

fn sdl() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let flags = sdl3_sys::init::SDL_INIT_EVENTS.0 | sdl3_sys::init::SDL_INIT_GAMEPAD.0;
        assert!(video::init_subsystem(flags), "{}", window::error());
    });
}

#[test]
fn scancodes_map_to_sdl_keycodes_and_names() {
    sdl();
    // (SDL_Scancode, SDL_Keycode, SDL_GetKeyName) on SDL's default US layout.
    let table = [
        (4, 'a' as u32, "A"),
        (29, 'z' as u32, "Z"),
        (30, '1' as u32, "1"),
        (39, '0' as u32, "0"),
        (40, 0x0d, "Return"),
        (41, 0x1b, "Escape"),
        (44, ' ' as u32, "Space"),
        (58, 0x4000_003a, "F1"),
        (225, 0x4000_00e1, "Left Shift"),
    ];
    for (scancode, key, name) in table {
        assert_eq!(keyboard::key_from_scancode(scancode, 0, true), key, "scancode {scancode}");
        assert_eq!(keyboard::key_name(key), name, "key {key:#x}");
        assert_eq!(keyboard::scancode_from_key(key), scancode, "key {key:#x}");
    }
    assert_eq!(keyboard::key_from_scancode(4, 0x0001, false), 'A' as u32, "shifted, outside key events");
    assert_eq!(keyboard::key_from_scancode(9999, 0, true), 0);
    assert_eq!(keyboard::key_name(0), "");
    assert_eq!(keyboard::scancode_name(4), "A");
}

/// Poll the next event the way the PollEvent slot does: straight into managed's struct.
fn poll_managed() -> Option<ManagedEvent> {
    let mut managed = ManagedEvent::default();
    // SAFETY: same size, and managed's alignment is at least SDL_Event's (tests/layout.rs).
    let sdl = unsafe { &mut *std::ptr::from_mut(&mut managed).cast::<window::Event>() };
    events::poll(sdl).then_some(managed)
}

#[test]
fn pushed_events_reach_managed_layout_intact() {
    sdl();
    while poll_managed().is_some() {}

    let mut key = events::new(SDL_EVENT_KEY_DOWN.0);
    key.key.windowID = SDL_WindowID(7);
    key.key.scancode = sdl3_sys::scancode::SDL_Scancode(4);
    key.key.key = sdl3_sys::keycode::SDL_Keycode('a' as u32);
    key.key.r#mod = sdl3_sys::keycode::SDL_Keymod(0x0040);
    key.key.down = true;
    key.key.repeat = true;
    assert!(events::push(&mut key), "{}", window::error());

    static TEXT: &std::ffi::CStr = c"h\u{e9}";
    let mut text = events::new(SDL_EVENT_TEXT_INPUT.0);
    text.text.windowID = SDL_WindowID(7);
    text.text.text = TEXT.as_ptr();
    assert!(events::push(&mut text));

    let mut wheel = events::new(SDL_EVENT_MOUSE_WHEEL.0);
    wheel.wheel.y = -2.5;
    wheel.wheel.direction = sdl3_sys::mouse::SDL_MOUSEWHEEL_FLIPPED;
    assert!(events::push(&mut wheel));

    let e = poll_managed().expect("key event");
    assert_eq!(e.type_(), 0x300);
    let k = e.key();
    assert_eq!((k.window_id, k.scancode, k.key, k.r#mod, k.down, k.repeat), (7, 4, 'a' as u32, 0x40, 1, 1));
    assert!(k.common.timestamp > 0, "SDL stamps pushed events");

    let e = poll_managed().expect("text event");
    assert_eq!(e.type_(), 0x303);
    // SAFETY: SDL leaves the pointer as pushed; TEXT is 'static.
    let s = unsafe { std::ffi::CStr::from_ptr(e.text().text as *const std::ffi::c_char) };
    assert_eq!(s.to_str().unwrap(), "h\u{e9}");

    let e = poll_managed().expect("wheel event");
    assert_eq!((e.type_(), e.wheel().y, e.wheel().direction), (0x403, -2.5, 1));
    assert!(poll_managed().is_none());
}

#[test]
fn gamepad_mappings_load_from_sdl_db_format() {
    sdl();
    let dir = std::env::temp_dir().join(format!("underbox-window-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gamecontrollerdb.txt");
    // As in SDL_GameControllerDB: lines without this platform's `platform:` field are skipped.
    let (here, other) = if cfg!(windows) { ("Windows", "Linux") } else { ("Linux", "Windows") };
    std::fs::write(
        &path,
        format!(
            "# Game Controller DB\n\n\
             03000000aaaa0000bbbb000000000000,Underbox Pad One,a:b0,b:b1,x:b2,y:b3,leftx:a0,lefty:a1,platform:{here},\n\
             03000000cccc0000dddd000000000000,Underbox Pad Two,a:b1,b:b0,start:b7,platform:{here},\n\
             03000000eeee0000ffff000000000000,Elsewhere Pad,a:b0,platform:{other},\n\
             03000000eeee0000ffff000000000000,No Platform Pad,a:b0,\n"
        ),
    )
    .unwrap();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(gamepad::add_mappings_from_file(&c_path), 2, "{}", window::error());

    let mapping = gamepad::mapping_for_guid(c"03000000aaaa0000bbbb000000000000").expect("mapping");
    assert!(mapping.contains("Underbox Pad One") && mapping.contains("a:b0"), "{mapping}");
    let mapping = gamepad::mapping_for_guid(c"03000000cccc0000dddd000000000000").expect("mapping");
    assert!(mapping.contains("start:b7"), "{mapping}");
    assert_eq!(gamepad::mapping_for_guid(c"03000000eeee0000ffff000000000000"), None);
    assert_eq!(gamepad::add_mapping(c"03000000cccc0000dddd000000000000,Underbox Pad Two,a:b0,"), 0, "update");
    assert!(gamepad::add_mapping(c"not a mapping") < 0);

    assert_eq!(gamepad::add_mappings_from_file(c"/nonexistent/gamecontrollerdb.txt"), -1);
    assert!(!window::error().is_empty());
    assert!(gamepad::open(12345).is_null(), "no such joystick");
    assert!(!gamepad::rumble(window::Gamepad::NULL, 1, 1, 1), "null gamepads are rejected, not dereferenced");
    std::fs::remove_dir_all(dir).ok();
}
