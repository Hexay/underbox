//! Managed's hand-written SDL mirrors (abi::structs, laid out as the CLR lays them out) must match
//! SDL3's structs exactly: events and display modes are written straight into managed memory.

use abi::structs as m;
use sdl3_sys::events::*;
use sdl3_sys::video::SDL_DisplayMode;
use std::mem::{offset_of, size_of};

macro_rules! same_layout {
    ($managed:ty, $sdl:ty { $($mf:ident $(. $mf2:ident)? = $sf:ident),* $(,)? }) => {{
        assert_eq!(size_of::<$managed>(), size_of::<$sdl>(), "size of {}", stringify!($sdl));
        $(assert_eq!(
            offset_of!($managed, $mf $(. $mf2)?),
            offset_of!($sdl, $sf),
            "{}.{}", stringify!($sdl), stringify!($sf)
        );)*
    }};
}

#[test]
fn event_union_is_128_bytes_with_type_first() {
    assert_eq!(size_of::<SDL_Event>(), 128);
    assert_eq!(size_of::<m::NativeEngine_Sdl_Event>(), 128);
    assert!(align_of::<m::NativeEngine_Sdl_Event>() >= align_of::<SDL_Event>());
}

#[test]
fn event_members_match_sdl3() {
    same_layout!(m::CommonEvent, SDL_CommonEvent { r#type = r#type, reserved = reserved, timestamp = timestamp });
    same_layout!(m::KeyboardEvent, SDL_KeyboardEvent {
        common.timestamp = timestamp, window_id = windowID, which = which, scancode = scancode,
        key = key, r#mod = r#mod, raw = raw, down = down, repeat = repeat,
    });
    same_layout!(m::TextInputEvent, SDL_TextInputEvent { window_id = windowID, text = text });
    same_layout!(m::TextEditingEvent, SDL_TextEditingEvent { window_id = windowID, text = text, start = start, length = length });
    same_layout!(m::MouseMotionEvent, SDL_MouseMotionEvent {
        window_id = windowID, which = which, state = state, x = x, y = y, x_rel = xrel, y_rel = yrel,
    });
    same_layout!(m::MouseButtonEvent, SDL_MouseButtonEvent {
        window_id = windowID, which = which, button = button, down = down, clicks = clicks, x = x, y = y,
    });
    same_layout!(m::MouseWheelEvent, SDL_MouseWheelEvent {
        window_id = windowID, which = which, x = x, y = y, direction = direction,
        mouse_x = mouse_x, mouse_y = mouse_y, integer_x = integer_x, integer_y = integer_y,
    });
    same_layout!(m::DropEvent, SDL_DropEvent { window_id = windowID, x = x, y = y, source = source, data = data });
    same_layout!(m::GamepadDeviceEvent, SDL_GamepadDeviceEvent { which = which });
    same_layout!(m::GamepadButtonEvent, SDL_GamepadButtonEvent { which = which, button = button, down = down });
    same_layout!(m::GamepadAxisEvent, SDL_GamepadAxisEvent { which = which, axis = axis, value = value });
}

#[test]
fn display_mode_and_dialog_filter_match_sdl3() {
    same_layout!(m::DisplayMode, SDL_DisplayMode {
        display = displayID, format = format, width = w, height = h, pixel_density = pixel_density,
        refresh_rate = refresh_rate, refresh_rate_numerator = refresh_rate_numerator,
        refresh_rate_denominator = refresh_rate_denominator, internal = internal,
    });
    same_layout!(m::DialogFileFilter, sdl3_sys::dialog::SDL_DialogFileFilter { name = name, pattern = pattern });
    same_layout!(m::Rect, sdl3_sys::rect::SDL_Rect { x = x, y = y, width = w, height = h });
    same_layout!(m::Point, sdl3_sys::rect::SDL_Point { x = x, y = y });
}

#[test]
fn managed_event_ids_are_sdl3s() {
    // NativeEngine.Sdl.EventType: managed switches on these raw values.
    let ids = [
        (0x100, SDL_EVENT_QUIT),
        (0x206, SDL_EVENT_WINDOW_RESIZED),
        (0x210, SDL_EVENT_WINDOW_CLOSE_REQUESTED),
        (0x214, SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED),
        (0x300, SDL_EVENT_KEY_DOWN),
        (0x303, SDL_EVENT_TEXT_INPUT),
        (0x403, SDL_EVENT_MOUSE_WHEEL),
        (0x650, SDL_EVENT_GAMEPAD_AXIS_MOTION),
        (0x654, SDL_EVENT_GAMEPAD_REMOVED),
        (0x1000, SDL_EVENT_DROP_FILE),
        (0x1003, SDL_EVENT_DROP_COMPLETE),
        (0x1101, SDL_EVENT_AUDIO_DEVICE_REMOVED),
    ];
    for (managed, sdl) in ids {
        assert_eq!(managed, sdl.0, "event {managed:#x}");
    }
}

#[test]
fn managed_flag_values_are_sdl3s() {
    use sdl3_sys::video::*;
    // NativeEngine.Sdl.WindowFlags and InitFlags.
    let flags = [
        (0x1, SDL_WINDOW_FULLSCREEN),
        (0x8, SDL_WINDOW_HIDDEN),
        (0x10, SDL_WINDOW_BORDERLESS),
        (0x20, SDL_WINDOW_RESIZABLE),
        (0x200, SDL_WINDOW_INPUT_FOCUS),
        (0x2000, SDL_WINDOW_HIGH_PIXEL_DENSITY),
        (0x4000, SDL_WINDOW_MOUSE_CAPTURE),
        (0x80000, SDL_WINDOW_POPUP_MENU),
        (0x1000_0000, SDL_WINDOW_VULKAN),
        (0x8000_0000, SDL_WINDOW_NOT_FOCUSABLE),
    ];
    for (managed, sdl) in flags {
        assert_eq!(managed, sdl.0, "flag {managed:#x}");
    }
    assert_eq!(0x2000, sdl3_sys::init::SDL_INIT_GAMEPAD.0);
}
