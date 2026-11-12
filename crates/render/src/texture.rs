//! Textures and render targets, with CPU upload and synchronous read-back.

use crate::{Frame, Gpu};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Clone, Debug)]
pub struct TextureDesc {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub mips: u32,
    pub render_target: bool,
    /// Array layers (six per cube); 1 for a plain 2D texture.
    pub layers: u32,
    /// Sampled as a cube (or cube array) rather than a 2D array.
    pub cube: bool,
}

impl TextureDesc {
    /// A sampleable colour render target that can be read back.
    pub fn render_target(label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        TextureDesc { label: label.into(), width, height, format, mips: 1, render_target: true, layers: 1, cube: false }
    }

    /// How shaders see it: 2D, 2D array, cube or cube array.
    pub fn dimension(&self) -> wgpu::TextureViewDimension {
        use wgpu::TextureViewDimension as D;
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

    fn usage(&self) -> wgpu::TextureUsages {
        let mut base = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
        // Compute writes (RWTexture2D) wherever the format allows them on every device.
        if self.format.guaranteed_format_features(wgpu::Features::empty()).allowed_usages.contains(wgpu::TextureUsages::STORAGE_BINDING) {
            base |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        if self.render_target { base | wgpu::TextureUsages::RENDER_ATTACHMENT } else { base }
    }
}

pub struct Texture {
    pub desc: TextureDesc,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// Mip 0 alone: attachments may not span mips.
    pub target_view: wgpu::TextureView,
    /// The same texels read through the format's sRGB twin (decoded on sample), for formats
    /// that have one (8-bit RGBA/BGRA and the BC formats).
    pub srgb_view: Option<wgpu::TextureView>,
}

/// `format`'s sRGB-decoding twin, when it has one and isn't sRGB already.
pub fn srgb_twin(format: wgpu::TextureFormat) -> Option<wgpu::TextureFormat> {
    let twin = format.add_srgb_suffix();
    (twin != format).then_some(twin)
}

impl Texture {
    pub fn new(gpu: &Gpu, desc: TextureDesc) -> Texture {
        let twin = srgb_twin(desc.format);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&desc.label),
            size: wgpu::Extent3d { depth_or_array_layers: desc.layers.max(1), ..extent(desc.width, desc.height) },
            mip_level_count: desc.mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.format,
            usage: desc.usage(),
            view_formats: twin.as_slice(),
        });
        let dimension = Some(desc.dimension());
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension, ..Default::default() });
        // Sampling only: sRGB formats can't be storage images, which the texture itself may allow.
        let srgb_view = twin.map(|f| texture.create_view(&wgpu::TextureViewDescriptor { format: Some(f), dimension, usage: Some(wgpu::TextureUsages::TEXTURE_BINDING), ..Default::default() }));
        let target_view = layer_view(&texture, 0);
        Texture { desc, texture, view, target_view, srgb_view }
    }

    pub fn dimension(&self) -> wgpu::TextureViewDimension {
        self.desc.dimension()
    }

    /// Mip 0 of one array layer (a cube face), as a 2D attachment.
    pub fn layer_target_view(&self, layer: u32) -> wgpu::TextureView {
        layer_view(&self.texture, layer)
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
    /// has a twin, else the raw view.
    pub fn sample_view(&self, srgb: bool) -> &wgpu::TextureView {
        self.srgb_view.as_ref().filter(|_| srgb).unwrap_or(&self.view)
    }

    /// [`Texture::sample_view`] of one mip only, as native binds a texture attribute set for a mip
    /// (managed's mip generation samples mip i while it writes mip i + 1).
    pub fn mip_sample_view(&self, srgb: bool, mip: u32) -> wgpu::TextureView {
        let format = srgb_twin(self.desc.format).filter(|_| srgb);
        let usage = format.map(|_| wgpu::TextureUsages::TEXTURE_BINDING);
        let mip = mip.min(self.texture.mip_level_count() - 1);
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            format,
            dimension: Some(self.dimension()),
            usage,
            base_mip_level: mip,
            mip_level_count: Some(1),
            ..Default::default()
        })
    }

    /// Bytes per texel of mip 0; `None` for block-compressed or depth-stencil formats.
    pub fn texel_size(&self) -> Option<u32> {
        let f = self.desc.format;
        if f.block_dimensions() != (1, 1) {
            return None;
        }
        f.block_copy_size(None)
    }

    /// Bytes of one tightly packed mip-0 layer.
    pub fn layer_size(&self) -> Option<usize> {
        Some((self.texel_size()? * self.desc.width * self.desc.height) as usize)
    }

    /// Replace mip 0 with tightly packed `data`: one layer, or every layer in order.
    pub fn upload(&self, gpu: &Gpu, data: &[u8]) {
        let (Some(texel), Some(layer)) = (self.texel_size(), self.layer_size()) else { return };
        let layers = if data.len() == layer * self.desc.layers.max(1) as usize { self.desc.layers.max(1) } else { 1 };
        // Queue writes land before the next submit: queued passes that read the old texels go first.
        gpu.flush();
        gpu.queue.write_texture(
            self.texture.as_image_copy(),
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.desc.width * texel), rows_per_image: Some(self.desc.height) },
            wgpu::Extent3d { depth_or_array_layers: layers, ..extent(self.desc.width, self.desc.height) },
        );
    }

    /// Fill mips 1 and up from `chain`: each level's layers tightly packed, level after level. Stops
    /// at the first level `chain` runs short of; returns how many levels it wrote.
    pub fn upload_smaller_mips(&self, gpu: &Gpu, chain: &[u8]) -> u32 {
        let Some(texel) = self.texel_size() else { return 0 };
        let layers = self.desc.layers.max(1);
        let mut at = 0;
        gpu.flush();
        for mip in 1..self.desc.mips.max(1) {
            let (w, h) = self.mip_size(mip);
            let len = (w * h * texel * layers) as usize;
            let Some(data) = chain.get(at..at + len) else { return mip - 1 };
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * texel), rows_per_image: Some(h) },
                wgpu::Extent3d { depth_or_array_layers: layers, ..extent(w, h) },
            );
            at += len;
        }
        self.desc.mips.max(1) - 1
    }

    /// Clear mip 0 with an empty render pass: to `color` for a colour format, or to `depth` (and
    /// stencil 0) for a depth format. The value for the other kind is ignored.
    pub fn clear(&self, gpu: &Gpu, color: wgpu::Color, depth: f32) {
        self.clear_layer(gpu, color, depth, 0);
    }

    /// [`Texture::clear`] of one array layer (a cube face).
    pub fn clear_layer(&self, gpu: &Gpu, color: wgpu::Color, depth: f32, layer: u32) {
        let view = (layer > 0).then(|| self.layer_target_view(layer));
        let view = view.as_ref().unwrap_or(&self.target_view);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let format = self.desc.format;
        let color_attachment = [Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color), store: wgpu::StoreOp::Store },
        })];
        let store = wgpu::StoreOp::Store;
        let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: format.has_depth_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(depth), store }),
            stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store }),
        };
        let is_depth = format.is_depth_stencil_format();
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: if is_depth { &[] } else { &color_attachment },
            depth_stencil_attachment: is_depth.then_some(depth_attachment),
            ..Default::default()
        });
        gpu.submit(encoder.finish());
    }

    /// Copy `[x, y, w, h]` of mip 0 into `dst` at `(dx, dy)`, in every array layer both have.
    /// False (nothing copied) when the formats differ or the rect falls outside either texture.
    pub fn copy_to(&self, gpu: &Gpu, dst: &Texture, [x, y, w, h]: [u32; 4], (dx, dy): (u32, u32)) -> bool {
        let fits = |t: &Texture, x: u32, y: u32| x + w <= t.desc.width && y + h <= t.desc.height;
        // Depth and stencil copies must cover whole subresources (wgpu validation).
        let whole = |t: &Texture| [x, y, dx, dy] == [0; 4] && (w, h) == (t.desc.width, t.desc.height);
        if self.desc.format.has_depth_aspect() && !(whole(self) && whole(dst)) {
            return false;
        }
        if self.desc.format != dst.desc.format || w == 0 || h == 0 || !fits(self, x, y) || !fits(dst, dx, dy) {
            return false;
        }
        fn at(texture: &wgpu::Texture, x: u32, y: u32) -> wgpu::TexelCopyTextureInfo<'_> {
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All }
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let layers = self.desc.layers.max(1).min(dst.desc.layers.max(1));
        encoder.copy_texture_to_texture(at(&self.texture, x, y), at(&dst.texture, dx, dy), wgpu::Extent3d { depth_or_array_layers: layers, ..extent(w, h) });
        gpu.submit(encoder.finish());
        true
    }

    /// Copy mip 0 back to the CPU, tightly packed. Blocks until the GPU is done.
    pub fn read_back(&self, gpu: &Gpu) -> Frame {
        self.read_region(gpu, 0, [0, 0, self.desc.width, self.desc.height]).expect("read-back needs an uncompressed colour format")
    }

    /// The size of `mip`.
    pub fn mip_size(&self, mip: u32) -> (u32, u32) {
        ((self.desc.width >> mip).max(1), (self.desc.height >> mip).max(1))
    }

    /// Copy `[x, y, w, h]` of `mip` back to the CPU, tightly packed; `None` for a format without
    /// a texel size or a rect outside the mip. Blocks until the GPU is done.
    pub fn read_region(&self, gpu: &Gpu, mip: u32, [x, y, w, h]: [u32; 4]) -> Option<Frame> {
        let texel = self.texel_size()?;
        let (mw, mh) = self.mip_size(mip);
        if mip >= self.desc.mips.max(1) || w == 0 || h == 0 || x + w > mw || y + h > mh {
            return None;
        }
        let row = w * texel;
        // Gotcha: buffer copies need rows padded to COPY_BYTES_PER_ROW_ALIGNMENT (256).
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: mip, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: None },
            },
            extent(w, h),
        );
        gpu.submit(encoder.finish());
        let bytes = crate::buffer::map_read(gpu, &staging);
        let mut pixels = Vec::with_capacity((row * h) as usize);
        for y in 0..h as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&bytes[start..start + row as usize]);
        }
        Some(Frame { width: w, height: h, texel_size: texel, pixels })
    }
}

fn layer_view(texture: &wgpu::Texture, layer: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        mip_level_count: Some(1),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

pub(crate) fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 }
}
