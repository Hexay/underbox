//! Copying a [`Texture`]'s texels back to the CPU.

use crate::texture::{extent, sample_aspect};
use crate::{Frame, Gpu, Texture};

impl Texture {
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
            wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: mip, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: sample_aspect(self.desc.format) },
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
