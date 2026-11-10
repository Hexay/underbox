//! The wgpu instance, adapter, device and queue.

use std::fmt;

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
    pub fn headless() -> Result<Gpu, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::from_env().unwrap_or_default(),
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        };
        let adapter = pollster::block_on(instance.request_adapter(&options)).map_err(GpuError::NoAdapter)?;
        let desc = wgpu::DeviceDescriptor {
            label: Some("underbox"),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&desc)).map_err(GpuError::Device)?;
        // wgpu's default handler panics, which would take a managed client down over one bad draw.
        device.on_uncaptured_error(std::sync::Arc::new(|e| eprintln!("wgpu error: {e}")));
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
            Some(e) => Err(e.to_string()),
            None => Ok(r),
        }
    }

    /// Block until every submitted command has finished (and map callbacks have run).
    pub fn wait_idle(&self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("wgpu device lost while waiting");
    }
}
