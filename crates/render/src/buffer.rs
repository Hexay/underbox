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
            // Managed GpuBuffers also feed instance streams (vertex) and the UI batcher's indices.
            BufferKind::Storage => U::STORAGE | U::INDIRECT | U::VERTEX | U::INDEX,
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

    /// Write `data` at `offset`, in order with queued work ([`Gpu::write_buffer`]). wgpu needs
    /// 4-byte multiples, so a ragged tail is zero-padded.
    pub fn write(&self, gpu: &Gpu, offset: u64, data: &[u8]) {
        gpu.write_buffer(&self.buffer, offset, data);
    }

    /// Fill the whole buffer with a repeated 32-bit value.
    pub fn fill(&self, gpu: &Gpu, value: u32) {
        let words = vec![value; (self.size() / 4) as usize];
        gpu.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&words));
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
    // The copy into `staging` may still be queued; a map before its submit would read nothing.
    gpu.flush();
    let (tx, rx) = std::sync::mpsc::channel();
    staging.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    // A hung GPU (Gpu::wait_idle logged it) reads zeros rather than waiting forever.
    let mapped = if gpu.wait_idle() { recv_polling(gpu, &rx) } else { rx.try_recv().ok() };
    match mapped {
        Some(Ok(())) => {
            let bytes = staging.get_mapped_range(..).expect("mapped range").to_vec();
            staging.unmap();
            bytes
        }
        Some(Err(e)) => {
            eprintln!("wgpu error: read-back failed: {e}");
            vec![0; staging.size() as usize]
        }
        None => vec![0; staging.size() as usize],
    }
}

/// The map callback after an idle wait. Gotcha: with threads sharing the device, another
/// thread's poll can fire it after our wait returned, so keep polling until it lands.
fn recv_polling<T>(gpu: &Gpu, rx: &std::sync::mpsc::Receiver<T>) -> Option<T> {
    for _ in 0..1000 {
        match rx.recv_timeout(std::time::Duration::from_millis(5)) {
            Ok(r) => return Some(r),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let _ = gpu.device.poll(wgpu::PollType::Poll);
            }
            Err(_) => return None,
        }
    }
    None
}

fn align4(n: u64) -> u64 {
    n.div_ceil(4) * 4
}
