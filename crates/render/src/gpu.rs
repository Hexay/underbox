//! The wgpu instance, adapter, device and queue.

use std::fmt;

/// What s&box's lit shaders need (bindless texture and sampler arrays, wave ops), taken when the
/// adapter has it.
const OPTIONAL: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY)
    .union(wgpu::Features::SUBGROUP)
    // DXC's storage images reflect read-write even when only written (no NonReadable), which
    // core WebGPU allows for few formats; the adapter's own format table allows more.
    .union(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);

/// One device and its queue. Everything else in this crate borrows it.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
}

#[derive(Debug)]
pub enum GpuError {
    NoAdapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::NoAdapter(e) => write!(f, "no wgpu adapter: {e}"),
            GpuError::Device(e) => write!(f, "wgpu device request failed: {e}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl Gpu {
    /// A device with no surface: everything renders to offscreen targets. Honours wgpu's env
    /// vars (`WGPU_BACKEND`, `WGPU_ADAPTER_NAME`, `WGPU_POWER_PREF`), so CI can pin a driver.
    ///
    /// Create one per process (tests share a static): the Vulkan loader frees a new instance's
    /// device-less ICDs without a lock that other instances' device calls take, which segfaults.
    pub fn headless() -> Result<Gpu, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::from_env().unwrap_or_default(),
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        };
        let adapter = pollster::block_on(instance.request_adapter(&options)).map_err(GpuError::NoAdapter)?;
        let (supported, limits) = (adapter.features(), adapter.limits());
        let desc = wgpu::DeviceDescriptor {
            label: Some("underbox"),
            required_features: supported & OPTIONAL,
            required_limits: wgpu::Limits {
                max_binding_array_elements_per_shader_stage: limits.max_binding_array_elements_per_shader_stage,
                max_binding_array_sampler_elements_per_shader_stage: limits.max_binding_array_sampler_elements_per_shader_stage,
                // s&box's lit shaders bind more per stage than wgpu's portable defaults (14+ storage buffers).
                max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
                max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage,
                max_samplers_per_shader_stage: limits.max_samplers_per_shader_stage,
                // downsample_spd writes seven mips.
                max_storage_textures_per_shader_stage: limits.max_storage_textures_per_shader_stage,
                max_uniform_buffers_per_shader_stage: limits.max_uniform_buffers_per_shader_stage,
                // envmap_filtering_cs runs 8×8×6 (one layer per cube face); D3D11-class devices take 1024.
                max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
                max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x,
                max_compute_workgroup_size_y: limits.max_compute_workgroup_size_y,
                max_compute_workgroup_size_z: limits.max_compute_workgroup_size_z,
                ..wgpu::Limits::default().using_resolution(limits)
            },
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&desc)).map_err(GpuError::Device)?;
        // wgpu's default handler panics, which would take a managed client down over one bad draw.
        // RUST_LIB_BACKTRACE=1 adds where the failing encoder or submit came from.
        device.on_uncaptured_error(std::sync::Arc::new(|e| {
            let at = std::backtrace::Backtrace::capture();
            let at = if at.status() == std::backtrace::BacktraceStatus::Captured { format!("\n{at}") } else { String::new() };
            eprintln!("wgpu error: {e}{at}");
        }));
        Ok(Gpu { info: adapter.get_info(), device, queue, instance, adapter })
    }

    /// `backend/adapter name`, e.g. `vulkan/llvmpipe (LLVM 20.1.2, 256 bits)`.
    pub fn describe(&self) -> String {
        format!("{}/{}", self.info.backend, self.info.name)
    }

    /// Run `f` (resource or pipeline creation) and report the validation error it raised, if any,
    /// instead of sending it to the uncaptured handler.
    pub fn checked<R>(&self, f: impl FnOnce() -> R) -> Result<R, String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let r = f();
        match pollster::block_on(scope.pop()) {
            // wgpu's Display is just "Validation Error"; the cause is in the description.
            Some(wgpu::Error::Validation { description, .. }) => Err(description),
            Some(e) => Err(e.to_string()),
            None => Ok(r),
        }
    }

    /// The largest device-local memory heap in bytes (what drivers report as VRAM; system RAM on
    /// a software or unified-memory device). wgpu doesn't expose heaps, so it asks Vulkan through
    /// wgpu-hal; `None` on other backends.
    pub fn device_local_bytes(&self) -> Option<u64> {
        // VK_MEMORY_HEAP_DEVICE_LOCAL_BIT.
        const DEVICE_LOCAL: u32 = 0x1;
        // SAFETY: the hal adapter is only read (no handle escapes the guard), and the physical
        // device belongs to the instance it is queried on.
        unsafe {
            let adapter = self.adapter.as_hal::<wgpu::hal::api::Vulkan>()?;
            let props = adapter.shared_instance().raw_instance().get_physical_device_memory_properties(adapter.raw_physical_device());
            props.memory_heaps_as_slice().iter().filter(|h| h.flags.as_raw() & DEVICE_LOCAL != 0).map(|h| h.size).max()
        }
    }

    /// Block until every submitted command has finished (and map callbacks have run).
    pub fn wait_idle(&self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("wgpu device lost while waiting");
    }
}
