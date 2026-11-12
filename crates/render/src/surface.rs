//! Presenting to a window. Frames always render offscreen (a swap chain's colour texture); with a
//! window, `present` copies that texture into the surface's current image. Surfaces come from a
//! `wgpu::SurfaceTarget` or raw window/display handles (SDL windows); without one, nothing here is
//! used and frames stay readable for hashing.

use crate::{Gpu, Texture};

pub use wgpu::SurfaceTarget;

/// Formats we render swap chains in, in order of preference; the surface must accept one of
/// them as a copy destination.
const FORMATS: [wgpu::TextureFormat; 2] = [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8Unorm];

pub struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

#[derive(Debug)]
pub enum SurfaceError {
    Create(wgpu::CreateSurfaceError),
    /// The adapter can't present to it, or offers none of [`FORMATS`] with copy-in.
    Unsupported,
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceError::Create(e) => write!(f, "surface creation failed: {e}"),
            SurfaceError::Unsupported => write!(f, "the adapter can't present to this surface"),
        }
    }
}

impl std::error::Error for SurfaceError {}

impl Surface {
    /// A surface on `target` sized `width × height`.
    pub fn new(gpu: &Gpu, target: impl Into<SurfaceTarget<'static>>, width: u32, height: u32, vsync: bool) -> Result<Surface, SurfaceError> {
        let surface = gpu.instance.create_surface(target).map_err(SurfaceError::Create)?;
        Surface::configure_new(gpu, surface, width, height, vsync)
    }

    /// A surface on raw window and display handles (an SDL window's, from the window crate).
    ///
    /// # Safety
    /// The handles must stay valid until the returned surface is dropped.
    pub unsafe fn from_raw(
        gpu: &Gpu,
        window: wgpu::rwh::RawWindowHandle,
        display: wgpu::rwh::RawDisplayHandle,
        width: u32,
        height: u32,
        vsync: bool,
    ) -> Result<Surface, SurfaceError> {
        let target = wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: window };
        // SAFETY: the caller keeps the handles alive for the surface's lifetime.
        let surface = unsafe { gpu.instance.create_surface_unsafe(target) }.map_err(SurfaceError::Create)?;
        Surface::configure_new(gpu, surface, width, height, vsync)
    }

    fn configure_new(gpu: &Gpu, surface: wgpu::Surface<'static>, width: u32, height: u32, vsync: bool) -> Result<Surface, SurfaceError> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = FORMATS.into_iter().find(|f| caps.formats.contains(f)).ok_or(SurfaceError::Unsupported)?;
        // TODO: surfaces without COPY_DST need a blit pass instead of a copy.
        if !caps.usages.contains(wgpu::TextureUsages::COPY_DST) {
            return Err(SurfaceError::Unsupported);
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        Ok(Surface { surface, config })
    }

    /// The format swap-chain colour textures must have to be copied in.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32, vsync: bool) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.config.present_mode = if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Copy `frame` (same format, at least the surface's size) to the window and present it.
    /// False when no image could be acquired (minimised, lost); the next frame retries.
    pub fn present(&self, gpu: &Gpu, frame: &Texture) -> bool {
        let image = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                self.surface.configure(&gpu.device, &self.config);
                return false;
            }
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let (w, h) = (self.config.width.min(frame.desc.width), self.config.height.min(frame.desc.height));
        encoder.copy_texture_to_texture(frame.texture.as_image_copy(), image.texture.as_image_copy(), crate::texture::extent(w, h));
        gpu.submit(encoder.finish());
        gpu.flush();
        gpu.queue.present(image);
        true
    }
}
