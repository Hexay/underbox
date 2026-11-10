//! The render device. A dedicated server keeps the empty device (managed sees no graphics and
//! skips GPU work); a client gets a wgpu device from the `render` crate. See device.rs and
//! docs/renderer.md.

pub mod attributes;
mod bindings;
mod buffers;
pub mod context;
pub mod device;
pub mod draw;
mod formats;
mod frames;
pub mod game_window;
pub mod image;
pub mod lighting;
pub mod material;
pub mod material_mode;
mod material_source;
pub mod readback;
pub mod scene;
pub mod shading;
pub mod stats;
pub mod swap_chain;
pub mod texture;
mod texture_generated;
mod tools;
pub mod vfx;

use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    // Web-streamed textures stream from Facepunch's CDN, which only real devices consume.
    bind!(r, abi::slots::glue_resources::SET_TEXTURE_WEB_STREAMING_STATE, |_, _| {});
    bind!(r, abi::slots::glue_resources::CANCEL_TEXTURE_WEB_STREAMING_PREFETCH, || 0);
    device::register(r);
    attributes::register(r);
    bindings::register(r);
    buffers::register(r);
    context::register(r);
    draw::immediate::register(r);
    draw::draw_call::register(r);
    game_window::register(r);
    image::register(r);
    material::register(r);
    material_mode::register(r);
    readback::register(r);
    scene::register(r);
    stats::register(r);
    swap_chain::register(r);
    texture::register(r);
    tools::register(r);
    vfx::register(r);
}
