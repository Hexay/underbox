//! The render device. Until the client renderer exists (docs/PLAN.md, M2) this is the empty
//! device Facepunch's engine runs dedicated servers on: managed sees no graphics and skips GPU work.

pub mod attributes;
pub mod image;
pub mod material;
mod material_source;
pub mod stats;
pub mod texture;
pub mod vfx;

use abi::slots::g_p_render_device as device;
use abi::{Registry, bind};

/// `NativeEngine.RenderDeviceAPI_t` (Sandbox.Engine/Core/Interop/NativeEngine/RenderAPI.cs).
#[allow(dead_code)]
#[repr(i64)]
enum RenderDeviceApi {
    Dx11 = 0,
    Vulkan = 1,
    Empty = 2,
}

pub fn register(r: &mut Registry) {
    bind!(r, device::GET_RENDER_DEVICE_API, || RenderDeviceApi::Empty as i64);
    bind!(r, device::FLUSH_PIPELINE_CACHE, || {});
    // Web-streamed textures stream from Facepunch's CDN, which only real devices consume.
    bind!(r, abi::slots::glue_resources::SET_TEXTURE_WEB_STREAMING_STATE, |_, _| {});
    bind!(r, abi::slots::glue_resources::CANCEL_TEXTURE_WEB_STREAMING_PREFETCH, || 0);
    attributes::register(r);
    image::register(r);
    material::register(r);
    stats::register(r);
    texture::register(r);
    vfx::register(r);
}
