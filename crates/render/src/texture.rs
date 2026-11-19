//! Textures and render targets: creation and views. Uploads, clears and copies are in texture_io.rs,
//! read-back in readback.rs.

use crate::Gpu;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Clone, Debug)]
pub struct TextureDesc {
    pub label: String,
    pub width: u32,
    pub height: u32,
    /// The image's format. An sRGB format here stores the image as real engine2 does its loaded
    /// textures: sampled through a linear view by default, through [`Texture::srgb_view`] on request.
    pub format: wgpu::TextureFormat,
    pub mips: u32,
    pub render_target: bool,
    /// Array layers (six per cube); 1 for a plain 2D texture; a volume's depth.
    pub layers: u32,
    /// Sampled as a cube (or cube array) rather than a 2D array.
    pub cube: bool,
    /// A 3D texture: never an attachment, its depth halves per mip.
    pub volume: bool,
}

impl TextureDesc {
    /// A sampleable colour render target that can be read back.
    pub fn render_target(label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        TextureDesc { label: label.into(), width, height, format, mips: 1, render_target: true, layers: 1, cube: false, volume: false }
    }

    /// How shaders see it: 2D, 2D array, cube, cube array or 3D.
    pub fn dimension(&self) -> wgpu::TextureViewDimension {
        use wgpu::TextureViewDimension as D;
        if self.volume {
            return D::D3;
        }
        match (self.cube && self.layers.is_multiple_of(6), self.layers) {
            (true, 6) => D::Cube,
            (true, _) => D::CubeArray,
            (false, 0 | 1) => D::D2,
            (false, _) => D::D2Array,
        }
    }

    pub fn depth(label: &str, width: u32, height: u32) -> Self {
        Self::render_target(label, width, height, DEPTH_FORMAT)
    }

    fn usage(&self, gpu: &Gpu) -> wgpu::TextureUsages {
        let mut base = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
        // Compute writes (RWTexture2D) wherever the device allows them for the format (the contact-shadow mask is R8).
        if gpu.format_features(self.format).allowed_usages.contains(wgpu::TextureUsages::STORAGE_BINDING) {
            base |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        // Mip generation draws each mip from the one above (mips.rs).
        let mip_target = self.mips > 1 && gpu.format_features(self.format).allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT);
        if (self.render_target || mip_target) && !self.volume { base | wgpu::TextureUsages::RENDER_ATTACHMENT } else { base }
    }
}

pub struct Texture {
    pub desc: TextureDesc,
    pub texture: wgpu::Texture,
    /// Every mip and layer through the format's linear form.
    pub view: wgpu::TextureView,
    /// Mip 0 alone: attachments may not span mips.
    pub target_view: wgpu::TextureView,
    /// The same texels read through the format's sRGB form (decoded on sample), for formats
    /// that have one (8-bit RGBA/BGRA and the BC formats).
    pub srgb_view: Option<wgpu::TextureView>,
    /// One-mip views made on request ([`Texture::mip_view`]), kept so bind groups that use them
    /// repeat across frames (bind_cache.rs).
    mip_views: Mutex<HashMap<MipViewKey, wgpu::TextureView>>,
    /// Bumped by every pass that targets it, every clear and every copy into it, not by
    /// storage-image writes (depth formats can't be storage images): [`Texture::writes`].
    writes: AtomicU64,
}

/// A one-mip view's format, mip and usage.
type MipViewKey = (wgpu::TextureFormat, u32, Option<wgpu::TextureUsages>);

/// `format`'s sRGB-decoding twin, when it has one and isn't sRGB already.
pub fn srgb_twin(format: wgpu::TextureFormat) -> Option<wgpu::TextureFormat> {
    let twin = format.add_srgb_suffix();
    (twin != format).then_some(twin)
}

impl Texture {
    pub fn new(gpu: &Gpu, desc: TextureDesc) -> Texture {
        let (linear, srgb) = (desc.format.remove_srgb_suffix(), desc.format.add_srgb_suffix());
        let other = if desc.format == linear { srgb } else { linear };
        let twin = (linear != srgb).then_some(other);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&desc.label),
            size: wgpu::Extent3d { depth_or_array_layers: desc.layers.max(1), ..extent(desc.width, desc.height) },
            mip_level_count: desc.mips.max(1),
            sample_count: 1,
            dimension: if desc.volume { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 },
            format: desc.format,
            usage: desc.usage(gpu),
            view_formats: twin.as_slice(),
        });
        let dimension = Some(desc.dimension());
        // Sampling only through the twin: sRGB formats can't be storage images, which the texture itself may allow.
        let sampled = |format| if format == desc.format { None } else { Some(wgpu::TextureUsages::TEXTURE_BINDING) };
        let format = (!desc.format.is_depth_stencil_format()).then_some(linear);
        let view = texture.create_view(&wgpu::TextureViewDescriptor { format, dimension, aspect: sample_aspect(desc.format), usage: sampled(linear), ..Default::default() });
        let srgb_view = (linear != srgb).then(|| texture.create_view(&wgpu::TextureViewDescriptor { format: Some(srgb), dimension, usage: sampled(srgb), ..Default::default() }));
        let target_view = match desc.volume {
            true => texture.create_view(&wgpu::TextureViewDescriptor { mip_level_count: Some(1), ..Default::default() }),
            false => subresource_view(&texture, desc.format, 0, 0),
        };
        Texture { desc, texture, view, target_view, srgb_view, mip_views: Mutex::default(), writes: AtomicU64::new(0) }
    }

    pub fn dimension(&self) -> wgpu::TextureViewDimension {
        self.desc.dimension()
    }

    /// How many writes have been recorded into the texture: equal readings bracket a span in
    /// which only storage images could have changed it.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Acquire)
    }

    /// Record a write: every pass, clear and copy into the texture calls this.
    pub fn wrote(&self) {
        self.writes.fetch_add(1, Ordering::AcqRel);
    }

    /// Mip 0 of one array layer (a cube face), as a 2D attachment.
    pub fn layer_target_view(&self, layer: u32) -> wgpu::TextureView {
        subresource_view(&self.texture, self.desc.format, 0, layer)
    }

    /// One mip of one array layer through `format` (the texture's or its twin), as a 2D attachment.
    pub fn subresource_target_view(&self, format: wgpu::TextureFormat, mip: u32, layer: u32) -> wgpu::TextureView {
        subresource_view(&self.texture, format, mip, layer)
    }

    /// Mip 0 as an attachment that encodes on write (native's `RENDER_SRGB` output), for render
    /// targets whose format has an sRGB twin.
    pub fn srgb_target_view(&self) -> Option<wgpu::TextureView> {
        let format = srgb_twin(self.desc.format).filter(|_| self.desc.render_target)?;
        Some(self.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
            mip_level_count: Some(1),
            array_layer_count: Some(1),
            ..Default::default()
        }))
    }

    /// The view a shader samples: decoded through [`Texture::srgb_view`] when `srgb` and the format
    /// has a twin, else the linear view.
    pub fn sample_view(&self, srgb: bool) -> &wgpu::TextureView {
        self.srgb_view.as_ref().filter(|_| srgb).unwrap_or(&self.view)
    }

    /// [`Texture::sample_view`] of one mip only, as native binds a texture attribute set for a mip
    /// (managed's mip generation samples mip i while it writes mip i + 1).
    pub fn mip_sample_view(&self, srgb: bool, mip: u32) -> wgpu::TextureView {
        let f = self.desc.format;
        let format = if srgb && self.srgb_view.is_some() { f.add_srgb_suffix() } else { f.remove_srgb_suffix() };
        let usage = (format != f).then_some(wgpu::TextureUsages::TEXTURE_BINDING);
        self.mip_view(format, mip, usage)
    }

    /// Mip `mip` alone (clamped to the last) through `format`, every layer, limited to `usage`.
    pub fn mip_view(&self, format: wgpu::TextureFormat, mip: u32, usage: Option<wgpu::TextureUsages>) -> wgpu::TextureView {
        let mip = mip.min(self.texture.mip_level_count() - 1);
        let mut views = self.mip_views.lock().unwrap();
        let view = views.entry((format, mip, usage)).or_insert_with(|| {
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                dimension: Some(self.dimension()),
                usage,
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        });
        view.clone()
    }

    /// The size of `mip`.
    pub fn mip_size(&self, mip: u32) -> (u32, u32) {
        ((self.desc.width >> mip).max(1), (self.desc.height >> mip).max(1))
    }

    /// Array layers of `mip`: every layer, or a volume's depth at that mip.
    pub fn mip_layers(&self, mip: u32) -> u32 {
        let layers = self.desc.layers.max(1);
        if self.desc.volume { (layers >> mip).max(1) } else { layers }
    }
}

/// What shaders sample and read-backs copy: a depth-stencil format's depth alone (a view or copy
/// of both aspects can't be bound or copied to a buffer).
pub fn sample_aspect(format: wgpu::TextureFormat) -> wgpu::TextureAspect {
    if format.has_depth_aspect() && format.has_stencil_aspect() { wgpu::TextureAspect::DepthOnly } else { wgpu::TextureAspect::All }
}

fn subresource_view(texture: &wgpu::Texture, format: wgpu::TextureFormat, mip: u32, layer: u32) -> wgpu::TextureView {
    let usage = (format != texture.format()).then_some(wgpu::TextureUsages::RENDER_ATTACHMENT);
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(format),
        dimension: Some(wgpu::TextureViewDimension::D2),
        usage,
        base_mip_level: mip,
        mip_level_count: Some(1),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

pub(crate) fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 }
}
