//! `globalSdl` input slots: the event queue, keyboard and text input, mouse and cursors, gamepads.

use super::{text, win, win_ret};
use abi::slots::global_sdl as sdl;
use abi::structs::{NativeEngine_Sdl_Event, Rect};
use abi::{RawPtr, Registry, StrRet, bind};
use window::{Cursor, Gamepad, events, gamepad, keyboard, mouse};

// Managed's event union is written in place as an SDL_Event (field layouts: window/tests/layout.rs).
const _: () = {
    assert!(size_of::<NativeEngine_Sdl_Event>() == size_of::<window::Event>());
    assert!(align_of::<NativeEngine_Sdl_Event>() >= align_of::<window::Event>());
};

pub(super) fn poll_event(out: &mut NativeEngine_Sdl_Event) -> bool {
    // SAFETY: same size, alignment at least SDL_Event's (asserted above), and both are plain-data
    // unions for which any bytes SDL writes are a valid value.
    let out = unsafe { &mut *std::ptr::from_mut(out).cast::<window::Event>() };
    events::poll(out)
}

fn mouse_state(global: bool, x: &mut f32, y: &mut f32) -> u32 {
    let (buttons, mx, my) = mouse::state(global);
    (*x, *y) = (mx, my);
    buttons
}

fn cursor(p: RawPtr<std::ffi::c_void>) -> Cursor {
    Cursor::from_ptr(p.as_ptr())
}

fn pad(p: RawPtr<std::ffi::c_void>) -> Gamepad {
    Gamepad::from_ptr(p.as_ptr())
}

pub fn register(r: &mut Registry) {
    bind!(r, sdl::POLL_EVENT, poll_event);
    // SAFETY: managed passes an `SDL_EventFilter` it keeps alive (a static unmanaged function).
    bind!(r, sdl::ADD_EVENT_WATCH, |cb: RawPtr<_>, ud: RawPtr<_>| unsafe { events::add_watch(cb.as_ptr(), ud.as_ptr()) });
    // SAFETY: as above; SDL only compares the pointers.
    bind!(r, sdl::REMOVE_EVENT_WATCH, |cb: RawPtr<_>, ud: RawPtr<_>| unsafe { events::remove_watch(cb.as_ptr(), ud.as_ptr()) });
    // SAFETY: managed passes the address of an Sdl.Event it holds (128 bytes).
    bind!(r, sdl::GET_WINDOW_FROM_EVENT, |e: RawPtr<_>| win_ret(unsafe { events::window_of(e.as_ptr()) }));

    bind!(r, sdl::GET_MOD_STATE, keyboard::mod_state);
    bind!(r, sdl::SET_MOD_STATE, keyboard::set_mod_state);
    bind!(r, sdl::GET_KEY_FROM_SCANCODE, keyboard::key_from_scancode);
    bind!(r, sdl::GET_KEY_NAME, |k| StrRet::from(keyboard::key_name(k)));
    bind!(r, sdl::GET_KEYBOARD_FOCUS, || win_ret(keyboard::focus()));
    bind!(r, sdl::START_TEXT_INPUT, |p| keyboard::start_text_input(win(p)));
    bind!(r, sdl::STOP_TEXT_INPUT, |p| keyboard::stop_text_input(win(p)));
    bind!(r, sdl::CLEAR_COMPOSITION, |p| keyboard::clear_composition(win(p)));
    bind!(r, sdl::SET_TEXT_INPUT_AREA, |p, a: &mut Rect, c| {
        keyboard::set_text_input_area(win(p), [a.x, a.y, a.width, a.height], c)
    });

    bind!(r, sdl::GET_MOUSE_STATE, |x, y| mouse_state(false, x, y));
    bind!(r, sdl::GET_GLOBAL_MOUSE_STATE, |x, y| mouse_state(true, x, y));
    bind!(r, sdl::CAPTURE_MOUSE, mouse::capture);
    bind!(r, sdl::GET_MOUSE_FOCUS, || win_ret(mouse::focus()));
    bind!(r, sdl::SET_WINDOW_RELATIVE_MOUSE_MODE, |p, v| mouse::set_relative_mode(win(p), v));
    bind!(r, sdl::GET_WINDOW_RELATIVE_MOUSE_MODE, |p| mouse::relative_mode(win(p)));
    bind!(r, sdl::WARP_MOUSE_IN_WINDOW, |p, x, y| mouse::warp(win(p), x, y));
    bind!(r, sdl::CREATE_SYSTEM_CURSOR, |k| RawPtr::from_raw(mouse::system_cursor(k as i32).as_ptr()));
    // SAFETY: managed passes a surface CreateSurfaceFrom returned and still holds.
    bind!(r, sdl::CREATE_COLOR_CURSOR, |s: RawPtr<_>, x, y| {
        RawPtr::from_raw(mouse::color_cursor(unsafe { window::surface::Surface::from_ptr(s.as_ptr()) }, x, y).as_ptr())
    });
    bind!(r, sdl::DESTROY_CURSOR, |c| mouse::destroy_cursor(cursor(c)));
    bind!(r, sdl::SET_CURSOR, |c| mouse::set_cursor(cursor(c)));
    bind!(r, sdl::SHOW_CURSOR, mouse::show_cursor);
    bind!(r, sdl::HIDE_CURSOR, mouse::hide_cursor);

    bind!(r, sdl::ADD_GAMEPAD_MAPPINGS_FROM_FILE, |path| gamepad::add_mappings_from_file(text(path)));
    bind!(r, sdl::SET_JOYSTICK_EVENTS_ENABLED, gamepad::set_joystick_events_enabled);
    bind!(r, sdl::SET_GAMEPAD_EVENTS_ENABLED, gamepad::set_gamepad_events_enabled);
    bind!(r, sdl::OPEN_GAMEPAD, |id| RawPtr::from_raw(gamepad::open(id).as_ptr()));
    bind!(r, sdl::GET_GAMEPAD_FROM_ID, |id| RawPtr::from_raw(gamepad::from_id(id).as_ptr()));
    bind!(r, sdl::CLOSE_GAMEPAD, |g| gamepad::close(pad(g)));
    bind!(r, sdl::GET_GAMEPAD_NAME, |g| StrRet::from(gamepad::name(pad(g))));
    bind!(r, sdl::GET_REAL_GAMEPAD_TYPE, |g| i64::from(gamepad::real_type(pad(g))));
    bind!(r, sdl::SET_GAMEPAD_LED, |g, rr, gg, b| gamepad::set_led(pad(g), rr, gg, b));
    bind!(r, sdl::RUMBLE_GAMEPAD, |g, lo, hi, ms| gamepad::rumble(pad(g), lo, hi, ms));
    bind!(r, sdl::RUMBLE_GAMEPAD_TRIGGERS, |g, lt, rt, ms| gamepad::rumble_triggers(pad(g), lt, rt, ms));
    bind!(r, sdl::GAMEPAD_HAS_SENSOR, |g, s| gamepad::has_sensor(pad(g), s as i32));
    bind!(r, sdl::SET_GAMEPAD_SENSOR_ENABLED, |g, s, on| gamepad::set_sensor_enabled(pad(g), s as i32, on));
    // SAFETY: managed passes a buffer of `count` floats.
    bind!(r, sdl::GET_GAMEPAD_SENSOR_DATA, |g, s, data: RawPtr<_>, n| unsafe {
        gamepad::sensor_data(pad(g), s as i32, data.as_ptr(), n)
    });
}
