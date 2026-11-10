//! The render device. A dedicated server keeps the empty device (managed sees no graphics and
//! skips GPU work); a client gets a wgpu device from the `render` crate. See device.rs and
//! docs/renderer.md.

pub mod attributes;
mod buffers;
pub mod context;
pub mod device;
mod formats;
pub mod image;
pub mod material;
mod material_source;
pub mod stats;
pub mod texture;
pub mod vfx;

use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    // Web-streamed textures stream from Facepunch's CDN, which only real devices consume.
    bind!(r, abi::slots::glue_resources::SET_TEXTURE_WEB_STREAMING_STATE, |_, _| {});
    bind!(r, abi::slots::glue_resources::CANCEL_TEXTURE_WEB_STREAMING_PREFETCH, || 0);
    device::register(r);
    attributes::register(r);
    buffers::register(r);
    context::register(r);
    image::register(r);
    material::register(r);
    stats::register(r);
    texture::register(r);
    vfx::register(r);
}
