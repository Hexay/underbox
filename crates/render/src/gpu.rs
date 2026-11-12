//! The wgpu instance, adapter, device and queue.

use crate::batch::{Batch, Slice};
use crate::bind_cache::BindCache;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// What s&box's lit shaders need (bindless texture and sampler arrays, wave ops), taken when the
/// adapter has it.
const OPTIONAL: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY)
    .union(wgpu::Features::SUBGROUP)
    .union(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER)
    // DXC's storage images reflect read-write even when only written (no NonReadable), which
    // core WebGPU allows for few formats; the adapter's own format table allows more.
    .union(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    // Block-compressed textures upload as stored, as real engine2 keeps them.
    .union(wgpu::Features::TEXTURE_COMPRESSION_BC)
    // 32-bit float textures behind filtering samplers, as the depth chain is gathered (MSAAUtils.hlsl).
    .union(wgpu::Features::FLOAT32_FILTERABLE)
    // Native's main depth (`D32FS8`).
    .union(wgpu::Features::DEPTH32FLOAT_STENCIL8);

/// One device and its queue. Everything else in this crate borrows it.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    hung: AtomicBool,
    wait_timeout_ms: AtomicU64,
    batch: Batch,
    bind_groups: BindCache,
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
        // Indirect-call validation stays on: managed (untrusted game code) fills indirect args.
        // `WGPU_VALIDATION_INDIRECT_CALL=0` turns it off, as the differential harness does.
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
        // A real driver's watchdog ends a hang this way; lavapipe has none (see wait_idle).
        device.set_device_lost_callback(|reason, message| eprintln!("wgpu error: device lost ({reason:?}): {message}"));
        Ok(Gpu {
            info: adapter.get_info(),
            device,
            queue,
            instance,
            adapter,
            hung: AtomicBool::new(false),
            wait_timeout_ms: AtomicU64::new(default_wait_timeout_ms()),
            batch: Batch::default(),
            bind_groups: BindCache::default(),
        })
    }

    /// `backend/adapter name`, e.g. `vulkan/llvmpipe (LLVM 20.1.2, 256 bits)`.
    pub fn describe(&self) -> String {
        format!("{}/{}", self.info.backend, self.info.name)
    }

    /// What the device allows for `format`: the adapter's own table when the device took it, else
    /// what wgpu guarantees.
    pub fn format_features(&self, format: wgpu::TextureFormat) -> wgpu::TextureFormatFeatures {
        let features = self.device.features();
        if features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            self.adapter.get_texture_format_features(format)
        } else {
            format.guaranteed_format_features(features)
        }
    }

    /// Whether BC1-BC7 textures can be created (and so uploaded as stored).
    pub fn supports_bc(&self) -> bool {
        self.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    }

    /// Run `f` (resource or pipeline creation) and report the validation error it raised, if any,
    /// instead of sending it to the uncaptured handler.
    pub fn checked<R>(&self, f: impl FnOnce() -> R) -> Result<R, String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let r = f();
        let err = match pollster::block_on(scope.pop()) {
            // wgpu's Display is just "Validation Error"; the cause is in the description.
            Some(wgpu::Error::Validation { description, .. }) => description,
            Some(e) => e.to_string(),
            None => return Ok(r),
        };
        self.bind_groups.clear();
        Err(err)
    }

    /// A bind group for `desc`: the one an earlier frame made from the same layout and resources,
    /// else a new one (crate::bind_cache).
    pub fn bind_group(&self, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup {
        self.bind_groups.get(&self.device, desc)
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

    /// Block until every submitted command has finished (and map callbacks have run), for at most
    /// [`Gpu::set_wait_timeout`]. A wait that times out marks the GPU hung (a runaway shader:
    /// lavapipe has no watchdog, so it never ends in device loss) and logs once; while hung,
    /// waits only poll and return false, until the queue drains. See docs/renderer.md.
    pub fn wait_idle(&self) -> bool {
        if self.hung.load(Ordering::Relaxed) {
            let drained = self.device.poll(wgpu::PollType::Poll).is_ok_and(|s| s.is_queue_empty());
            if drained {
                self.hung.store(false, Ordering::Relaxed);
                eprintln!("wgpu: the GPU caught up after a hang");
            }
            return drained;
        }
        self.flush();
        let timeout = Duration::from_millis(self.wait_timeout_ms.load(Ordering::Relaxed));
        match timed(|| "waiting for the GPU".into(), || self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(timeout) })) {
            Ok(_) => true,
            Err(e) => {
                self.hung.store(true, Ordering::Relaxed);
                eprintln!("wgpu error: GPU hang: submitted work didn't finish within {timeout:?} ({e}); waits stop blocking and read-backs return zeros until it does");
                false
            }
        }
    }

    /// Queue `commands` for the next [`Gpu::flush`] (crate::batch).
    pub fn submit(&self, commands: wgpu::CommandBuffer) {
        self.batch.push(commands);
    }

    /// Write the staged uploads and submit everything queued, or drop it while the GPU is hung:
    /// on lavapipe a submit behind a running shader blocks until it ends, which would wedge the
    /// caller after all.
    pub fn flush(&self) {
        self.flush_batch(false);
    }

    /// [`Gpu::flush`] at the end of this thread's frame: its next uploads reuse its upload buffer.
    pub fn end_frame(&self) {
        self.flush_batch(true);
        self.bind_groups.end_frame();
    }

    fn flush_batch(&self, end_frame: bool) {
        self.batch.drain(&self.queue, end_frame, |commands| {
            if (self.is_hung() && !self.wait_idle()) || commands.is_empty() {
                return;
            }
            self.queue.submit(commands);
        });
    }

    /// `bytes` zero-padded to `size` in this thread's upload buffer, aligned for any binding.
    pub fn upload(&self, bytes: &[u8], size: u64) -> Slice {
        let l = self.device.limits();
        let align = u64::from(l.min_uniform_buffer_offset_alignment.max(l.min_storage_buffer_offset_alignment));
        self.batch.upload(&self.device, &self.queue, bytes, size, align)
    }

    /// Write `data` into `dst` at `offset` (4-byte aligned), ordered with the queued command buffers.
    pub fn write_buffer(&self, dst: &wgpu::Buffer, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.batch.write(&self.device, &self.queue, dst, offset, data);
    }

    /// Whether the last wait timed out and the queue hasn't drained since.
    pub fn is_hung(&self) -> bool {
        self.hung.load(Ordering::Relaxed)
    }

    /// How long [`Gpu::wait_idle`] blocks before calling the GPU hung. Starts at
    /// `UNDERBOX_GPU_TIMEOUT` seconds, else 30.
    pub fn set_wait_timeout(&self, timeout: Duration) {
        self.wait_timeout_ms.store(timeout.as_millis().max(1) as u64, Ordering::Relaxed);
    }
}

/// Runs `f`, logging `what` when it takes a second or more: pipeline compiles and GPU waits are
/// what stall a frame on lavapipe.
pub fn timed<R>(what: impl FnOnce() -> String, f: impl FnOnce() -> R) -> R {
    let start = std::time::Instant::now();
    let r = f();
    let took = start.elapsed();
    if took >= Duration::from_secs(1) {
        eprintln!("wgpu: slow: {} took {:.1} s", what(), took.as_secs_f32());
    }
    r
}

fn default_wait_timeout_ms() -> u64 {
    let secs = std::env::var("UNDERBOX_GPU_TIMEOUT").ok().and_then(|v| v.parse::<f64>().ok()).filter(|s| *s > 0.0).unwrap_or(30.0);
    (secs * 1000.0) as u64
}
