//! The render device. A dedicated server keeps the empty device (managed sees no graphics and
//! skips GPU work); a client gets a wgpu device from the `render` crate. See device.rs and
//! docs/renderer.md.

pub mod attributes;
pub mod bindless;
mod bindings;
mod buffers;
mod compute;
pub mod context;
pub mod device;
pub mod draw;
pub mod fog;
pub mod fog_managed;
pub mod fog_volume;
mod formats;
mod frames;
pub mod game_window;
pub mod image;
mod image_convert;
pub mod exposure;
pub mod lighting;
pub mod material;
pub mod material_mode;
mod material_source;
pub mod readback;
pub mod samplers;
pub mod scene;
pub(crate) mod scene_utils;
pub mod shading;
pub mod sheet;
pub mod skin;
pub mod stats;
pub(crate) mod submits;
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
    bind!(r, abi::slots::glue_resources::IS_TEXTURE_WEB_STREAMING_ACTIVE, || false);
    bind!(r, abi::slots::glue_resources::HAS_TEXTURE_WEB_STREAMING_REQUESTS, || false);
    // Nothing to fetch: the prefetch is done as soon as it's asked for.
    bind!(r, abi::slots::glue_resources::PREFETCH_TEXTURE_WEB_STREAMING, |_, _, _, _| true);
    device::register(r);
    attributes::register(r);
    bindings::register(r);
    buffers::register(r);
    compute::register(r);
    context::register(r);
    draw::immediate::register(r);
    draw::draw_call::register(r);
    game_window::register(r);
    image::register(r);
    material::register(r);
    material_mode::register(r);
    readback::register(r);
    scene::register(r);
    scene_utils::register(r);
    sheet::register(r);
    skin::register(r);
    bind!(r, abi::slots::image_loader::CONVERT_IMAGE_FORMAT, image_convert::convert);
    stats::register(r);
    swap_chain::register(r);
    texture::register(r);
    tools::register(r);
    vfx::register(r);
}
