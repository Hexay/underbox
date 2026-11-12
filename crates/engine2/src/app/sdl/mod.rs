//! `globalSdl`: SDL3's API one function per slot, answered by real SDL3 through the `window`
//! crate (why SDL and not winit: docs/renderer.md, "Windows and input"). A dedicated server never
//! brings SDL video up: the gate below refuses it, and its event queue reads empty.

mod input;
mod system;
mod windows;

use abi::slots::global_sdl as sdl;
use abi::{RawPtr, Registry, bind};
use std::ffi::{CStr, c_void};
use window::Window;

/// SDL's own: our earlier 1 ms spin tail cost a server about 1 ms of CPU per frame
/// (docs/perf-physics.md).
fn delay_precise(ns: u64) {
    window::delay_precise(ns);
}

fn video_allowed() -> bool {
    !crate::app::dict::current_config().dedicated_server
}

fn win(p: RawPtr<c_void>) -> Window {
    Window::from_ptr(p.as_ptr())
}

fn win_ret(w: Window) -> RawPtr<c_void> {
    RawPtr::from_raw(w.as_ptr())
}

/// Managed marshals a null string for `null`; SDL wants "" there.
fn text(s: Option<&CStr>) -> &CStr {
    s.unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    window::video::set_gate(video_allowed);
    bind!(r, sdl::DELAY_PRECISE, delay_precise);
    input::register(r);
    system::register(r);
    windows::register(r);
}

#[cfg(test)]
mod tests;
