//! GPU buffers: vertex, index, uniform and storage, with synchronous read-back.

use crate::Gpu;
use wgpu::BufferUsages as U;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Vertex,
    Index,
    Uniform,
    Storage,
}

impl BufferKind {
    fn usage(self) -> U {
        let copy = U::COPY_DST | U::COPY_SRC;
        copy | match self {
            BufferKind::Vertex => U::VERTEX | U::STORAGE,
            BufferKind::Index => U::INDEX | U::STORAGE,
            BufferKind::Uniform => U::UNIFORM,
            // Vertex too: managed's instance-id buffers feed the instance stream.
            BufferKind::Storage => U::STORAGE | U::INDIRECT | U::VERTEX,
        }
    }
}

pub struct Buffer {
    pub kind: BufferKind,
    pub buffer: wgpu::Buffer,
}

impl Buffer {
    /// A zeroed buffer of `size` bytes (rounded up to wgpu's 4-byte copy alignment).
    pub fn new(gpu: &Gpu, label: &str, kind: BufferKind, size: u64) -> Buffer {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: align4(size.max(4)),
            usage: kind.usage(),
            mapped_at_creation: false,
        });
        Buffer { kind, buffer }
    }

    pub fn with_data(gpu: &Gpu, label: &str, kind: BufferKind, data: &[u8]) -> Buffer {
        let b = Buffer::new(gpu, label, kind, data.len() as u64);
        b.write(gpu, 0, data);
        b
    }

    pub fn size(&self) -> u64 {
        self.buffer.size()
    }

    /// Write `data` at `offset`. wgpu needs 4-byte multiples, so a ragged tail is zero-padded.
    pub fn write(&self, gpu: &Gpu, offset: u64, data: &[u8]) {
        if data.len().is_multiple_of(4) {
            gpu.queue.write_buffer(&self.buffer, offset, data);
        } else {
            let mut padded = data.to_vec();
            padded.resize(align4(data.len() as u64) as usize, 0);
            gpu.queue.write_buffer(&self.buffer, offset, &padded);
        }
    }

    /// Fill the whole buffer with a repeated 32-bit value.
    pub fn fill(&self, gpu: &Gpu, value: u32) {
        let words = vec![value; (self.size() / 4) as usize];
        gpu.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&words));
    }

    /// Copy `len` bytes from this buffer at `src_offset` to `dst` at `dst_offset` (4-byte aligned).
    pub fn copy_to(&self, gpu: &Gpu, dst: &Buffer, src_offset: u64, dst_offset: u64, len: u64) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.buffer, src_offset, &dst.buffer, dst_offset, len);
        gpu.submit(encoder.finish());
    }

    /// Copy `len` bytes at `offset` back to the CPU. Blocks until the GPU is done.
    pub fn read(&self, gpu: &Gpu, offset: u64, len: u64) -> Vec<u8> {
        // Copies must start and end on 4-byte boundaries; widen, then trim.
        let start = offset / 4 * 4;
        let end = align4(offset + len).min(self.size());
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer readback"),
            size: (end - start).max(4),
            usage: U::COPY_DST | U::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.buffer, start, &staging, 0, end - start);
        gpu.submit(encoder.finish());
        let bytes = map_read(gpu, &staging);
        let skip = (offset - start) as usize;
        bytes[skip..(skip + len as usize).min(bytes.len())].to_vec()
    }
}

/// Map a `MAP_READ` buffer, wait for it and copy its contents out.
pub(crate) fn map_read(gpu: &Gpu, staging: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    staging.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    gpu.wait_idle();
    // Not mapped yet means a hung GPU (Gpu::wait_idle logged it): zeros rather than a wait forever.
    match rx.try_recv() {
        Ok(Ok(())) => {
            let bytes = staging.get_mapped_range(..).expect("mapped range").to_vec();
            staging.unmap();
            bytes
        }
        Ok(Err(e)) => {
            eprintln!("wgpu error: read-back failed: {e}");
            vec![0; staging.size() as usize]
        }
        Err(_) => vec![0; staging.size() as usize],
    }
}

fn align4(n: u64) -> u64 {
    n.div_ceil(4) * 4
}
