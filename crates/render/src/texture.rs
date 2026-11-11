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
}

impl TextureDesc {
    /// A sampleable colour render target that can be read back.
    pub fn render_target(label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        TextureDesc { label: label.into(), width, height, format, mips: 1, render_target: true }
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
}

impl Texture {
    pub fn new(gpu: &Gpu, desc: TextureDesc) -> Texture {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&desc.label),
            size: extent(desc.width, desc.height),
            mip_level_count: desc.mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.format,
            usage: desc.usage(),
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target_view = texture.create_view(&wgpu::TextureViewDescriptor { mip_level_count: Some(1), ..Default::default() });
        Texture { desc, texture, view, target_view }
    }

    /// Bytes per texel of mip 0; `None` for block-compressed or depth-stencil formats.
    pub fn texel_size(&self) -> Option<u32> {
        let f = self.desc.format;
        if f.block_dimensions() != (1, 1) {
            return None;
        }
        f.block_copy_size(None)
    }

    /// Replace mip 0 with tightly packed `data`.
    pub fn upload(&self, gpu: &Gpu, data: &[u8]) {
        let Some(texel) = self.texel_size() else { return };
        gpu.queue.write_texture(
            self.texture.as_image_copy(),
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.desc.width * texel), rows_per_image: None },
            extent(self.desc.width, self.desc.height),
        );
    }

    /// Clear mip 0 with an empty render pass: to `color` for a colour format, or to `depth` (and
    /// stencil 0) for a depth format. The value for the other kind is ignored.
    pub fn clear(&self, gpu: &Gpu, color: wgpu::Color, depth: f32) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let format = self.desc.format;
        let color_attachment = [Some(wgpu::RenderPassColorAttachment {
            view: &self.target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color), store: wgpu::StoreOp::Store },
        })];
        let store = wgpu::StoreOp::Store;
        let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
            view: &self.target_view,
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
        gpu.queue.submit([encoder.finish()]);
    }

    /// Copy `[x, y, w, h]` of mip 0 into `dst` at `(dx, dy)`. False (nothing copied) when the
    /// formats differ or the rect falls outside either texture.
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
        encoder.copy_texture_to_texture(at(&self.texture, x, y), at(&dst.texture, dx, dy), extent(w, h));
        gpu.queue.submit([encoder.finish()]);
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
        gpu.queue.submit([encoder.finish()]);
        let bytes = crate::buffer::map_read(gpu, &staging);
        let mut pixels = Vec::with_capacity((row * h) as usize);
        for y in 0..h as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&bytes[start..start + row as usize]);
        }
        Some(Frame { width: w, height: h, texel_size: texel, pixels })
    }
}

pub(crate) fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 }
}
