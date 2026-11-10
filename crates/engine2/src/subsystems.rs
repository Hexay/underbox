//! Every subsystem's slot bindings, gathered once for `igen_engine`.
//!
//! Gotcha: slots marked `[nogc]` run under SuppressGCTransition and must never call a managed
//! callback; route diagnostics through `abi::rt::log` (stderr), not managed's logger.

use abi::{Registry, bind, slots};
use std::ffi::CStr;

pub fn register(r: &mut Registry) {
    bind!(r, slots::ERROR, managed_error);
    abi::bind_casts(r);
    crate::app::register(r);
    crate::render::register(r);
    crate::containers::register(r);
    crate::fs::register(r);
    crate::compiled::register(r);
    crate::audio::register(r);
}

/// Slot 0: managed reports an interop failure (a binding it couldn't make).
fn managed_error(message: Option<&CStr>) {
    let message = message.map_or_else(|| "(no message)".into(), CStr::to_string_lossy);
    abi::rt::log::log(abi::rt::log::Level::Error, &format!("managed interop error: {message}"));
}
