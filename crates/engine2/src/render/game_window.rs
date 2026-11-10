//! The game window native renders the client frame for (`GameWindowGlue`), and that frame:
//! inside `SourceEngineFrame` native has managed render (`EngineLoop.OnClientOutput`) and then
//! present (`GameWindow.Present`, which presents the window's swap chain).

use abi::callbacks::{sandbox_engine_game_window as game_window, sandbox_engine_loop as engine_loop};
use abi::slots::game_window_glue as glue;
use abi::{Registry, bind};
use std::sync::Mutex;

/// The attached window and swap chain, as addresses.
static ATTACHED: Mutex<Option<(usize, usize)>> = Mutex::new(None);

/// One client frame, if a game window is attached and managed has the callbacks registered.
/// A dedicated server never attaches a window, so its frames stay empty.
pub fn frame() {
    let registered = abi::rt::callbacks::is_set(10) && abi::rt::callbacks::is_set(15);
    if ATTACHED.lock().unwrap().is_none() || !registered {
        return;
    }
    engine_loop::on_client_output();
    game_window::present();
}

pub fn register(r: &mut Registry) {
    // Plat windows are the SDL windows themselves here.
    bind!(r, glue::FROM_NATIVE_HANDLE, |w| w);
    bind!(r, glue::ATTACH, |w, chain| *ATTACHED.lock().unwrap() = Some((w.as_ptr() as usize, chain.value)));
    bind!(r, glue::SET_RENDER_TARGET, |w, chain| *ATTACHED.lock().unwrap() = Some((w.as_ptr() as usize, chain.value)));
    bind!(r, glue::DETACH, |_| *ATTACHED.lock().unwrap() = None);
}
