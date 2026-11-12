//! Moving texels in and out of a [`Texture`]: uploads (texel or 4x4 block rows), clears, copies
//! and synchronous read-backs.

use crate::texture::extent;
use crate::{Frame, Gpu, Texture};

impl Texture {
    /// Bytes per texel of mip 0; `None` for block-compressed or depth-stencil formats.
    pub fn texel_size(&self) -> Option<u32> {
        let f = self.desc.format;
        if f.block_dimensions() != (1, 1) {
            return None;
        }
        f.block_copy_size(None)
    }

    /// (bytes per row, rows) of `mip` packed tightly: rows of texels, or of 4x4 blocks.
    fn rows(&self, mip: u32) -> Option<(u32, u32)> {
        let f = self.desc.format;
        let (bw, bh) = f.block_dimensions();
        let block = f.block_copy_size(None)?;
        let (w, h) = self.mip_size(mip);
        Some((w.div_ceil(bw) * block, h.div_ceil(bh)))
    }

    /// The extent a copy of all of `mip` covers: whole blocks, so a 2x2 BC mip copies 4x4.
    fn copy_extent(&self, mip: u32, layers: u32) -> wgpu::Extent3d {
        let (bw, bh) = self.desc.format.block_dimensions();
        let (w, h) = self.mip_size(mip);
        wgpu::Extent3d { width: w.next_multiple_of(bw), height: h.next_multiple_of(bh), depth_or_array_layers: layers }
    }

    /// Bytes of one tightly packed mip-0 layer.
    pub fn layer_size(&self) -> Option<usize> {
        let (row, rows) = self.rows(0)?;
        Some((row * rows) as usize)
    }

    fn write_mip(&self, gpu: &Gpu, mip: u32, layers: u32, data: &[u8]) {
        let Some((row, rows)) = self.rows(mip) else { return };
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows) },
            self.copy_extent(mip, layers),
        );
    }

    /// Replace mip 0 with tightly packed `data`: one layer, or every layer in order.
    pub fn upload(&self, gpu: &Gpu, data: &[u8]) {
        let Some(layer) = self.layer_size() else { return };
        let layers = if data.len() == layer * self.desc.layers.max(1) as usize { self.desc.layers.max(1) } else { 1 };
        // Queue writes land before the next submit: queued passes that read the old texels go first.
        gpu.flush();
        self.write_mip(gpu, 0, layers, data);
    }

    /// Fill mips 1 and up from `chain`: each level's layers tightly packed, level after level. Stops
    /// at the first level `chain` runs short of; returns how many levels it wrote.
    pub fn upload_smaller_mips(&self, gpu: &Gpu, chain: &[u8]) -> u32 {
        let layers = self.desc.layers.max(1);
        let mut at = 0;
        gpu.flush();
        for mip in 1..self.desc.mips.max(1) {
            let Some((row, rows)) = self.rows(mip) else { return 0 };
            let len = (row * rows * layers) as usize;
            let Some(data) = chain.get(at..at + len) else { return mip - 1 };
            self.write_mip(gpu, mip, layers, data);
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
    /// False (nothing copied) when the formats differ (an sRGB twin counts as the same) or the rect
    /// falls outside either texture.
    pub fn copy_to(&self, gpu: &Gpu, dst: &Texture, [x, y, w, h]: [u32; 4], (dx, dy): (u32, u32)) -> bool {
        let fits = |t: &Texture, x: u32, y: u32| x + w <= t.desc.width && y + h <= t.desc.height;
        // Depth and stencil copies must cover whole subresources (wgpu validation).
        let whole = |t: &Texture| [x, y, dx, dy] == [0; 4] && (w, h) == (t.desc.width, t.desc.height);
        if self.desc.format.has_depth_aspect() && !(whole(self) && whole(dst)) {
            return false;
        }
        let same = self.desc.format.remove_srgb_suffix() == dst.desc.format.remove_srgb_suffix();
        if !same || w == 0 || h == 0 || !fits(self, x, y) || !fits(dst, dx, dy) {
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
