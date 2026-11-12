//! The buffers bind.rs binds: instance transforms and block bytes, placed in the frame's upload
//! buffer (render/batch.rs) unless a shader writes them.

use super::bind::Instance;
use render::wgpu;

/// `g_TransformBuffer` entries: rows 0–2 of the world matrix, alpha, packed RGB tint, padding.
pub(super) fn transforms(instances: &[Instance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(instances.len().max(1) * 64);
    for i in instances {
        let rows = i.world.transpose().to_cols_array();
        out.extend(rows[..12].iter().flat_map(|f| f.to_le_bytes()));
        let [r, g, b, _] = i.tint.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u32);
        out.extend(i.tint[3].to_le_bytes());
        out.extend(((r << 16) | (g << 8) | b).to_le_bytes());
        out.extend([0u8; 8]);
    }
    out.resize(out.len().max(64), 0);
    out
}

/// Managed's bytes for a whole block, sized to the block.
pub(super) fn block_bytes(bytes: &[u8], size: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.resize(size.max(16) as usize, 0);
    out
}

fn padded_size(bytes: &[u8]) -> u64 {
    (bytes.len() as u64).next_multiple_of(16).max(16)
}

/// `bytes` as a binding: read-only ones in the upload buffer, which is bound read-only, so a
/// shader-writable buffer gets its own.
pub(super) fn buffer(gpu: &render::Gpu, label: &str, writable: bool, bytes: &[u8]) -> render::Slice {
    if !writable {
        return gpu.upload(bytes, padded_size(bytes));
    }
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: padded_size(bytes), usage, mapped_at_creation: false });
    gpu.write_buffer(&b, 0, bytes);
    whole(b)
}

pub(super) fn whole(buffer: wgpu::Buffer) -> render::Slice {
    render::Slice { size: buffer.size(), buffer, offset: 0 }
}
