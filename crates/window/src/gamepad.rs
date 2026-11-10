//! Gamepads: SDL's mapping database (managed loads `core/cfg/gamecontrollerdb.txt`, SDL's own
//! format), device lifetime by `SDL_JoystickID`, rumble, LEDs and motion sensors.

use crate::{Gamepad, owned};
use sdl3_sys::gamepad::*;
use sdl3_sys::joystick::{SDL_JoystickID, SDL_SetJoystickEventsEnabled};
use sdl3_sys::sensor::SDL_SensorType;
use std::ffi::{CStr, c_void};

/// SDL_AddGamepadMappingsFromFile: how many mappings were added, or -1 (error set).
pub fn add_mappings_from_file(path: &CStr) -> i32 {
    // SAFETY: path is NUL-terminated.
    unsafe { SDL_AddGamepadMappingsFromFile(path.as_ptr()) }
}

/// SDL_AddGamepadMapping for one `GUID,name,mapping` line: 1 added, 0 updated, -1 error.
pub fn add_mapping(line: &CStr) -> i32 {
    // SAFETY: line is NUL-terminated.
    unsafe { SDL_AddGamepadMapping(line.as_ptr()) }
}

/// The mapping SDL holds for a joystick GUID string, if any.
pub fn mapping_for_guid(guid: &CStr) -> Option<String> {
    // SAFETY: guid is NUL-terminated; the returned string is SDL-allocated and freed here.
    unsafe {
        let guid = sdl3_sys::guid::SDL_StringToGUID(guid.as_ptr());
        let mapping = SDL_GetGamepadMappingForGUID(guid);
        let text = owned(mapping);
        crate::free(mapping.cast());
        text
    }
}

pub fn set_joystick_events_enabled(enabled: bool) {
    // SAFETY: plain SDL call.
    unsafe { SDL_SetJoystickEventsEnabled(enabled) }
}

pub fn set_gamepad_events_enabled(enabled: bool) {
    // SAFETY: plain SDL call.
    unsafe { SDL_SetGamepadEventsEnabled(enabled) }
}

pub fn open(id: u32) -> Gamepad {
    // SAFETY: plain SDL call; unknown ids fail with an error.
    Gamepad(unsafe { SDL_OpenGamepad(SDL_JoystickID(id)) })
}

pub fn from_id(id: u32) -> Gamepad {
    // SAFETY: plain query.
    Gamepad(unsafe { SDL_GetGamepadFromID(SDL_JoystickID(id)) })
}

pub fn close(gamepad: Gamepad) {
    // SAFETY: SDL validates the gamepad (CHECK_GAMEPAD_MAGIC).
    unsafe { SDL_CloseGamepad(gamepad.0) }
}

pub fn name(gamepad: Gamepad) -> Option<String> {
    // SAFETY: SDL validates the gamepad.
    owned(unsafe { SDL_GetGamepadName(gamepad.0) })
}

pub fn real_type(gamepad: Gamepad) -> i32 {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_GetRealGamepadType(gamepad.0) }.0
}

pub fn set_led(gamepad: Gamepad, r: u8, g: u8, b: u8) -> bool {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_SetGamepadLED(gamepad.0, r, g, b) }
}

pub fn rumble(gamepad: Gamepad, low: u16, high: u16, ms: u32) -> bool {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_RumbleGamepad(gamepad.0, low, high, ms) }
}

pub fn rumble_triggers(gamepad: Gamepad, left: u16, right: u16, ms: u32) -> bool {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_RumbleGamepadTriggers(gamepad.0, left, right, ms) }
}

pub fn has_sensor(gamepad: Gamepad, sensor: i32) -> bool {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_GamepadHasSensor(gamepad.0, SDL_SensorType(sensor)) }
}

pub fn set_sensor_enabled(gamepad: Gamepad, sensor: i32, enabled: bool) -> bool {
    // SAFETY: SDL validates the gamepad.
    unsafe { SDL_SetGamepadSensorEnabled(gamepad.0, SDL_SensorType(sensor), enabled) }
}

/// SDL_GetGamepadSensorData into `data` (`count` floats).
///
/// # Safety
/// `data` must have room for `count` floats.
pub unsafe fn sensor_data(gamepad: Gamepad, sensor: i32, data: *mut c_void, count: i32) -> bool {
    unsafe { SDL_GetGamepadSensorData(gamepad.0, SDL_SensorType(sensor), data.cast(), count) }
}
