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
        let base = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
        if self.render_target { base | wgpu::TextureUsages::RENDER_ATTACHMENT } else { base }
    }
}

pub struct Texture {
    pub desc: TextureDesc,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
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
        Texture { desc, texture, view }
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

    /// Clear mip 0 to an RGBA colour with an empty render pass.
    pub fn clear(&self, gpu: &Gpu, color: wgpu::Color) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        gpu.queue.submit([encoder.finish()]);
    }

    /// Copy mip 0 back to the CPU, tightly packed. Blocks until the GPU is done.
    pub fn read_back(&self, gpu: &Gpu) -> Frame {
        let texel = self.texel_size().expect("read-back needs an uncompressed colour format");
        let (w, h) = (self.desc.width, self.desc.height);
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
            self.texture.as_image_copy(),
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
        Frame { width: w, height: h, texel_size: texel, pixels }
    }
}

pub(crate) fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 }
}
