//! Moving texels into and between [`Texture`]s: uploads (texel or 4x4 block rows), fills, clears
//! and copies. Read-back is in readback.rs.

use crate::texture::sample_aspect;
use crate::{Gpu, Texture};

/// One mip of one array layer (a cube face, a volume's depth slice).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subresource {
    pub mip: u32,
    pub layer: u32,
}

impl Texture {
    /// Bytes per texel of mip 0 (a depth-stencil format's depth); `None` for block-compressed
    /// formats and depth that can't be copied.
    pub fn texel_size(&self) -> Option<u32> {
        let f = self.desc.format;
        if f.block_dimensions() != (1, 1) {
            return None;
        }
        f.block_copy_size(Some(sample_aspect(f)))
    }

    /// (bytes per row, rows) of `mip` packed tightly: rows of texels, or of 4x4 blocks.
    fn rows(&self, mip: u32) -> Option<(u32, u32)> {
        let f = self.desc.format;
        let (bw, bh) = f.block_dimensions();
        let block = f.block_copy_size(Some(sample_aspect(f)))?;
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

    /// Replace the `[w, h, layers]` box of mip 0 at `[x, y, first layer]` with tightly packed texel rows.
    pub fn upload_region(&self, gpu: &Gpu, [x, y, z]: [u32; 3], [w, h, layers]: [u32; 3], data: &[u8]) {
        let Some(texel) = self.texel_size() else { return };
        gpu.flush();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * texel), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
        );
    }

    /// Fill mips 1 and up from `chain`: each level's layers tightly packed, level after level. Stops
    /// at the first level `chain` runs short of; returns how many levels it wrote.
    pub fn upload_smaller_mips(&self, gpu: &Gpu, chain: &[u8]) -> u32 {
        let mut at = 0;
        gpu.flush();
        for mip in 1..self.desc.mips.max(1) {
            let Some((row, rows)) = self.rows(mip) else { return 0 };
            let layers = self.mip_layers(mip);
            let len = (row * rows * layers) as usize;
            let Some(data) = chain.get(at..at + len) else { return mip - 1 };
            self.write_mip(gpu, mip, layers, data);
            at += len;
        }
        self.desc.mips.max(1) - 1
    }

    /// Every texel of every mip and layer set to the bytes of `texel` (`ClearTextureUint`). False
    /// for depth, block-compressed formats and a texel of the wrong size.
    pub fn fill(&self, gpu: &Gpu, texel: &[u8]) -> bool {
        if self.desc.format.is_depth_stencil_format() || self.texel_size() != Some(texel.len() as u32) {
            return false;
        }
        gpu.flush();
        for mip in 0..self.desc.mips.max(1) {
            let (w, h) = self.mip_size(mip);
            let layers = self.mip_layers(mip);
            self.write_mip(gpu, mip, layers, &texel.repeat((w * h * layers) as usize));
        }
        true
    }

    /// Clear mip 0 with an empty render pass: to `color` for a colour format, or to `depth` (and
    /// stencil 0) for a depth format. The value for the other kind is ignored.
    pub fn clear(&self, gpu: &Gpu, color: wgpu::Color, depth: f32) {
        self.clear_layer(gpu, color, depth, 0);
    }

    /// [`Texture::clear`] of one array layer (a cube face). Volumes aren't attachments: no-op.
    pub fn clear_layer(&self, gpu: &Gpu, color: wgpu::Color, depth: f32, layer: u32) {
        if self.desc.volume {
            return;
        }
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
    pub fn copy_to(&self, gpu: &Gpu, dst: &Texture, rect: [u32; 4], to: (u32, u32)) -> bool {
        let layers = self.desc.layers.max(1).min(dst.desc.layers.max(1));
        self.copy_region(gpu, dst, (Subresource::default(), Subresource::default()), layers, rect, to)
    }

    /// Copy `[x, y, w, h]` of `layers` layers from `src` into `dst` at `(dx, dy)`, between the
    /// (mip, first layer) pairs of `subs`. False (nothing copied) when the formats differ (an sRGB
    /// twin counts as the same), a mip or layer is missing, or the rect falls outside either mip.
    pub fn copy_region(&self, gpu: &Gpu, dst: &Texture, (src, to): (Subresource, Subresource), layers: u32, [x, y, w, h]: [u32; 4], (dx, dy): (u32, u32)) -> bool {
        let format = self.desc.format;
        let (bw, bh) = format.block_dimensions();
        let whole = |t: &Texture, s: Subresource, x: u32, y: u32| [x, y] == [0, 0] && (w, h) == t.mip_size(s.mip);
        let fits = |t: &Texture, s: Subresource, x: u32, y: u32| {
            let (mw, mh) = t.mip_size(s.mip);
            let aligned = whole(t, s, x, y) || (x.is_multiple_of(bw) && y.is_multiple_of(bh) && w.is_multiple_of(bw) && h.is_multiple_of(bh));
            s.mip < t.desc.mips.max(1) && s.layer + layers <= t.mip_layers(s.mip) && x + w <= mw && y + h <= mh && aligned
        };
        // Depth and stencil copies must cover whole subresources (wgpu validation).
        if format.has_depth_aspect() && !(whole(self, src, x, y) && whole(dst, to, dx, dy)) {
            return false;
        }
        let same = format.remove_srgb_suffix() == dst.desc.format.remove_srgb_suffix();
        if !same || w == 0 || h == 0 || layers == 0 || !fits(self, src, x, y) || !fits(dst, to, dx, dy) {
            return false;
        }
        let at = |texture, s: Subresource, x, y| wgpu::TexelCopyTextureInfo { texture, mip_level: s.mip, origin: wgpu::Origin3d { x, y, z: s.layer }, aspect: wgpu::TextureAspect::All };
        let size = wgpu::Extent3d { width: w.next_multiple_of(bw), height: h.next_multiple_of(bh), depth_or_array_layers: layers };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(at(&self.texture, src, x, y), at(&dst.texture, to, dx, dy), size);
        gpu.submit(encoder.finish());
        true
    }

    /// Copy mip 0 into array layer `layer` (a cube face) of `dst`, mip 0. False (nothing copied)
    /// unless both have the same size and format and `dst` has that layer.
    pub fn copy_to_layer(&self, gpu: &Gpu, dst: &Texture, layer: u32) -> bool {
        let (a, b) = (&self.desc, &dst.desc);
        if a.format.remove_srgb_suffix() != b.format.remove_srgb_suffix() || (a.width, a.height) != (b.width, b.height) || layer >= b.layers.max(1) {
            return false;
        }
        fn at(texture: &wgpu::Texture, z: u32) -> wgpu::TexelCopyTextureInfo<'_> {
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z }, aspect: wgpu::TextureAspect::All }
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(at(&self.texture, 0), at(&dst.texture, layer), extent(a.width, a.height));
        gpu.submit(encoder.finish());
        true
    }
}
