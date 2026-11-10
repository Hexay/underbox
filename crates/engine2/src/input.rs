//! `g_pInputSystem`: whether the game window has input focus (managed's WindowInput reports it).
//! The editor's play-window registration is a no-op: there's no editor host here.

use abi::slots::g_p_input_system as input;
use abi::{Registry, bind};
use std::sync::atomic::{AtomicBool, Ordering};

static GAME_FOCUS: AtomicBool = AtomicBool::new(true);

/// Whether the game window has focus, as managed last said.
pub fn game_focus() -> bool {
    GAME_FOCUS.load(Ordering::Relaxed)
}

pub fn register(r: &mut Registry) {
    bind!(r, input::SET_GAME_FOCUS, |focused| GAME_FOCUS.store(focused, Ordering::Relaxed));
    bind!(r, input::REGISTER_WINDOW_WITH_SDL, |_| {});
    bind!(r, input::UNREGISTER_WINDOW_FROM_SDL, |_| {});
    bind!(r, input::SET_EDITOR_MAIN_WINDOW, |_| {});
}
