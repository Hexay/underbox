//! The render device. Until the client renderer exists (docs/PLAN.md, M2) this is the empty
//! device Facepunch's engine runs dedicated servers on: managed sees no graphics and skips GPU work.

pub mod attributes;
pub mod image;
pub mod material;

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
    attributes::register(r);
    image::register(r);
    material::register(r);
}
