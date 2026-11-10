//! Subsystem lifetime, hints and the video gate.
//!
//! Video comes up lazily, on the first call that needs a display, so a process that never asks
//! for one (the dedicated server) never connects to X11/Wayland. The embedder installs a gate
//! ([`set_gate`]) that can refuse video outright. Without a usable display SDL falls back to its
//! `offscreen` driver: windows then exist without a native surface (see [`crate::handle`]).

use crate::set_error;
use sdl3_sys::hints::{SDL_HINT_VIDEO_DRIVER, SDL_SetHint};
use sdl3_sys::init::{SDL_INIT_VIDEO, SDL_InitFlags, SDL_InitSubSystem, SDL_QuitSubSystem, SDL_WasInit};
use std::ffi::CStr;
use std::sync::{Mutex, OnceLock};

/// The driver used when no display can be opened.
pub const FALLBACK_DRIVER: &CStr = c"offscreen";

static GATE: OnceLock<fn() -> bool> = OnceLock::new();
static VIDEO: Mutex<State> = Mutex::new(State::Untried);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Untried,
    Up,
    Failed,
}

/// Install the check that decides whether this process may use video at all (first call wins).
pub fn set_gate(allowed: fn() -> bool) {
    let _ = GATE.set(allowed);
}

fn gate_allows() -> bool {
    GATE.get().is_none_or(|allowed| allowed())
}

/// Bring the video subsystem up once, falling back to the offscreen driver. A refusal by the
/// gate is not remembered (the app's role can still change); a failed init is.
pub fn ensure() -> bool {
    let mut state = VIDEO.lock().unwrap_or_else(|e| e.into_inner());
    match *state {
        State::Up => return true,
        State::Failed => return set_error("video is unavailable (initialization failed earlier)"),
        State::Untried => {}
    }
    if !gate_allows() {
        return set_error("video is unavailable in this app (dedicated server)");
    }
    *state = if init_video() { State::Up } else { State::Failed };
    *state == State::Up
}

fn init_video() -> bool {
    // SAFETY: plain SDL calls; hint name and value are NUL-terminated constants.
    unsafe {
        if SDL_InitSubSystem(SDL_INIT_VIDEO) {
            return true;
        }
        let reason = crate::error();
        eprintln!("window: no display ({reason}); using SDL's {FALLBACK_DRIVER:?} video driver");
        SDL_SetHint(SDL_HINT_VIDEO_DRIVER, FALLBACK_DRIVER.as_ptr()) && SDL_InitSubSystem(SDL_INIT_VIDEO)
    }
}

/// Whether video is up (without trying to bring it up).
pub fn is_up() -> bool {
    // SAFETY: plain query.
    unsafe { SDL_WasInit(SDL_INIT_VIDEO) == SDL_INIT_VIDEO }
}

/// SDL_InitSubSystem. Video goes through [`ensure`] so the gate and fallback apply.
pub fn init_subsystem(flags: u32) -> bool {
    let video = flags & SDL_INIT_VIDEO.0 != 0;
    if video && !ensure() {
        return false;
    }
    let rest = SDL_InitFlags(flags & !SDL_INIT_VIDEO.0);
    // SAFETY: plain SDL call.
    rest.0 == 0 || unsafe { SDL_InitSubSystem(rest) }
}

/// SDL_QuitSubSystem. Video stays up once started: windows managed still holds would dangle.
pub fn quit_subsystem(flags: u32) {
    let rest = SDL_InitFlags(flags & !SDL_INIT_VIDEO.0);
    // SAFETY: plain SDL call; SDL ref-counts subsystems.
    unsafe { SDL_QuitSubSystem(rest) }
}

/// Whether any SDL subsystem is up (an uninitialised SDL has no event queue to read).
pub fn any_up() -> bool {
    // SAFETY: plain query.
    unsafe { SDL_WasInit(SDL_InitFlags(0)).0 != 0 }
}

/// SDL_SetHint. Works before init; managed sets hints before creating windows.
pub fn set_hint(name: &CStr, value: &CStr) -> bool {
    // SAFETY: both NUL-terminated; SDL copies them.
    unsafe { SDL_SetHint(name.as_ptr(), value.as_ptr()) }
}

/// SDL_GetCurrentVideoDriver: "x11", "wayland", "windows", "offscreen", ... or `None` before video.
pub fn current_driver() -> Option<String> {
    // SAFETY: plain query.
    crate::owned(unsafe { sdl3_sys::video::SDL_GetCurrentVideoDriver() })
}

pub fn disable_screen_saver() -> bool {
    // SAFETY: plain SDL call.
    ensure() && unsafe { sdl3_sys::video::SDL_DisableScreenSaver() }
}

pub fn enable_screen_saver() -> bool {
    // SAFETY: plain SDL call.
    ensure() && unsafe { sdl3_sys::video::SDL_EnableScreenSaver() }
}
