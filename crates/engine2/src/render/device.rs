//! Who owns the GPU. A dedicated server (or a unit-test app) keeps the empty device Facepunch's
//! engine runs servers on; any other app creates one headless wgpu device on first use, and
//! every render slot reaches it through [`gpu`]. See docs/renderer.md, "Device ownership".

use crate::app::dict;
use abi::slots::g_p_render_device as device;
use abi::{Registry, bind};
use std::sync::OnceLock;

/// `NativeEngine.RenderDeviceAPI_t` (Sandbox.Engine/Core/Interop/NativeEngine/RenderAPI.cs).
#[allow(dead_code)]
#[repr(i64)]
enum RenderDeviceApi {
    Dx11 = 0,
    Vulkan = 1,
    Empty = 2,
}

static GPU: OnceLock<Option<render::Gpu>> = OnceLock::new();

/// Whether this process should have a GPU: not a server or test app, and not opted out with
/// `UNDERBOX_RENDER=empty`.
pub fn wants_gpu() -> bool {
    let config = dict::current_config();
    let opted_out = std::env::var("UNDERBOX_RENDER").is_ok_and(|v| v == "empty");
    !config.dedicated_server && !config.test_mode && !opted_out
}

/// The device, if one was created. Never creates one: servers stay GPU-free.
pub fn gpu() -> Option<&'static render::Gpu> {
    GPU.get().and_then(Option::as_ref)
}

/// Create the device (once). A failure is logged and leaves the empty device in place.
pub fn create() -> Option<&'static render::Gpu> {
    GPU.get_or_init(|| match render::Gpu::headless() {
        Ok(gpu) => {
            abi::rt::log::log(abi::rt::log::Level::Info, &format!("render device: {}", gpu.describe()));
            Some(gpu)
        }
        Err(e) => {
            abi::rt::log::log(abi::rt::log::Level::Warn, &format!("render device unavailable, using the empty device: {e}"));
            None
        }
    })
    .as_ref()
}

/// Managed's `Graphics.Initialize` asks this first; the answer gates all of its GPU work.
fn render_device_api() -> i64 {
    let api = if wants_gpu() && create().is_some() { RenderDeviceApi::Vulkan } else { RenderDeviceApi::Empty };
    api as i64
}

/// `SystemInfo.SetGpu` at the tail of `SourceEngineInit`, as native does: managed's first-run
/// graphics preset (`RenderSettings.DetectPreset`) reads it. Without a device, nothing is reported.
// TODO: device-local memory; wgpu doesn't expose it, so 0 is reported and the preset detects Low.
pub fn report_to_managed() {
    let Some(gpu) = gpu() else { return };
    let info = &gpu.info;
    let integrated = matches!(info.device_type, render::wgpu::DeviceType::IntegratedGpu | render::wgpu::DeviceType::Cpu);
    let (name, driver) = (std::ffi::CString::new(info.name.clone()).ok(), std::ffi::CString::new(info.driver_info.clone()).ok());
    abi::callbacks::sandbox_engine_system_info::set_gpu(name.as_deref(), driver.as_deref(), 0, integrated);
}

fn wait_idle() {
    if let Some(gpu) = gpu() {
        gpu.wait_idle();
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, device::GET_RENDER_DEVICE_API, render_device_api);
    bind!(r, device::FLUSH_PIPELINE_CACHE, || {});
    bind!(r, device::FLUSH, wait_idle);
    bind!(r, device::FORCE_FLUSH_GPU, |_swap_chain| wait_idle());
    // Display lists are submitted as each context is; there is no deferred list to open.
    bind!(r, device::BEGIN_SUBMITTING_DISPLAY_LISTS, || {});
    bind!(r, device::BEGIN_SUBMITTING_DISPLAY_LISTS_1, |_swap_chain| {});
    bind!(r, device::SET_SUBMIT_TAG, |_| {});
    bind!(r, device::IS_USING32_BIT_DEPTH_BUFFER, || true);
    bind!(r, device::IS_RAY_TRACING_SUPPORTED, || false);
    // Markers name the scopes UNDERBOX_SUBMITS=log prints.
    bind!(r, device::ARE_DEBUG_MARKERS_ENABLED, crate::render::submits::enabled);
    bind!(r, device::SET_FORCE_PRELOAD_STREAMING_DATA, |_| {});
    bind!(r, device::GET_VIDEO_MEMORY_INFO, |a, b, c| {
        (*a, *b, *c) = (0, 0, 0);
    });
}
