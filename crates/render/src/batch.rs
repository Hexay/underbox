//! Batched uploads and submits, as real engine2 does them: a frame's per-draw constants, instance
//! streams and buffer writes are staged in one upload buffer and written with one copy per flush,
//! and command buffers are queued and submitted together. A flush happens before anything that
//! waits on the GPU or presents; see docs/renderer.md, "Batched uploads".

use std::sync::Mutex;
use wgpu::BufferUsages as U;

/// Smallest upload buffer; a larger upload gets one of its own size.
const CHUNK: u64 = 4 << 20;

/// What the upload buffer can be bound as. Never written by the GPU, so every use is a read.
const USAGE: U = U::UNIFORM.union(U::STORAGE).union(U::VERTEX).union(U::INDEX).union(U::INDIRECT).union(U::COPY_SRC).union(U::COPY_DST);

/// A range of an upload buffer, valid until the frame ends.
#[derive(Clone, Debug)]
pub struct Slice {
    pub buffer: wgpu::Buffer,
    pub offset: u64,
    pub size: u64,
}

impl Slice {
    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.buffer, offset: self.offset, size: wgpu::BufferSize::new(self.size) })
    }
}

#[derive(Default)]
struct State {
    chunk: Option<wgpu::Buffer>,
    /// Bytes of `chunk` from `written` up to the cursor, not yet written to the GPU.
    staged: Vec<u8>,
    written: u64,
    /// Ordered buffer writes (copies out of the upload buffer) since the last queued command buffer.
    copies: Option<wgpu::CommandEncoder>,
    pending: Vec<wgpu::CommandBuffer>,
}

impl State {
    fn cursor(&self) -> u64 {
        self.written + self.staged.len() as u64
    }

    /// Write what is staged into the current chunk (ordered before the next submit).
    fn write_staged(&mut self, queue: &wgpu::Queue) {
        if let Some(chunk) = &self.chunk
            && !self.staged.is_empty()
        {
            queue.write_buffer(chunk, self.written, &self.staged);
            self.written += self.staged.len() as u64;
            self.staged.clear();
        }
    }

    fn close_copies(&mut self) {
        if let Some(e) = self.copies.take() {
            self.pending.push(e.finish());
        }
    }
}

#[derive(Default)]
pub(crate) struct Batch {
    state: Mutex<State>,
}

impl Batch {
    /// Stage `bytes`, zero-padded to `size`, at an offset aligned to `align`.
    pub fn upload(&self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], size: u64, align: u64) -> Slice {
        let size = size.max(bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(4);
        let mut s = self.state.lock().unwrap();
        let mut offset = s.cursor().next_multiple_of(align);
        let room = s.chunk.as_ref().map_or(0, wgpu::Buffer::size);
        if offset + size > room {
            // Earlier slices of the old chunk stay valid: what they hold is written, and bind groups keep it alive.
            s.write_staged(queue);
            s.chunk = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("uploads"), size: CHUNK.max(size), usage: USAGE, mapped_at_creation: false }));
            s.written = 0;
            offset = 0;
        }
        let at = (offset - s.written) as usize;
        s.staged.resize(at, 0);
        s.staged.extend_from_slice(bytes);
        s.staged.resize(at + size as usize, 0);
        Slice { buffer: s.chunk.clone().expect("chunk"), offset, size }
    }

    /// Record a copy of `src` into `dst` at `offset`, ordered with the queued command buffers.
    pub fn copy(&self, device: &wgpu::Device, src: &Slice, dst: &wgpu::Buffer, offset: u64, len: u64) {
        let mut s = self.state.lock().unwrap();
        let e = s.copies.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("buffer writes") }));
        e.copy_buffer_to_buffer(&src.buffer, src.offset, dst, offset, len);
    }

    pub fn push(&self, commands: wgpu::CommandBuffer) {
        let mut s = self.state.lock().unwrap();
        s.close_copies();
        s.pending.push(commands);
    }

    /// Write the staged uploads and hand the queued command buffers, in order, to `submit`. With
    /// `end_frame`, later uploads reuse the chunk from its start: everything that read it is queued.
    /// Gotcha: `submit` runs under the lock, or two threads' flushes could submit out of order.
    pub fn drain(&self, queue: &wgpu::Queue, end_frame: bool, submit: impl FnOnce(Vec<wgpu::CommandBuffer>)) {
        let mut s = self.state.lock().unwrap();
        s.write_staged(queue);
        s.close_copies();
        if end_frame {
            s.written = 0;
        }
        submit(std::mem::take(&mut s.pending));
    }
}
