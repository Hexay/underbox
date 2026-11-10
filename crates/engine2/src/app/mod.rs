//! The application: startup configuration, the process, the engine's lifecycle.

mod diagnostics;
pub mod dict;
pub mod platform;
mod sdl;

pub fn register(r: &mut abi::Registry) {
    dict::register(r);
    platform::register(r);
    sdl::register(r);
    diagnostics::register(r);
}
