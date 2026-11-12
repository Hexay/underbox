//! Batched uploads and submits, as real engine2 does them: a frame's per-draw constants, instance
//! streams and buffer writes are staged in one upload buffer and written with one copy per flush,
//! and command buffers are queued and submitted together. A flush happens before anything that
//! waits on the GPU or presents.
//!
//! Each thread stages into its own upload buffer, and a frame end rewinds only the ending thread's
//! (so bind groups keyed by buffer and offset repeat; crate::bind_cache). With one shared buffer,
//! a present on one thread rewound it under another thread's slices whose commands weren't queued
//! yet, and that thread's draws read the next frame's bytes: the engine2 render tests flaked
//! whenever shader translation got faster (tests/batch.rs is the repro).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::thread::ThreadId;
use wgpu::BufferUsages as U;

/// Smallest upload buffer; a larger upload gets one of its own size.
const CHUNK: u64 = 4 << 20;

/// What the upload buffer can be bound as. Never written by the GPU, so every use is a read.
const USAGE: U = U::UNIFORM.union(U::STORAGE).union(U::VERTEX).union(U::INDEX).union(U::INDIRECT).union(U::COPY_SRC).union(U::COPY_DST);

thread_local! {
    /// Dropped when the thread exits, so its chunk can go (`Chunk::owner`).
    static ALIVE: Arc<()> = Arc::new(());
}

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

/// One thread's upload buffer (per thread: see the module docs).
struct Chunk {
    buffer: Option<wgpu::Buffer>,
    /// Bytes of `buffer` from `written` up to the cursor, not yet written to the GPU.
    staged: Vec<u8>,
    written: u64,
    owner: Weak<()>,
}

impl Chunk {
    fn cursor(&self) -> u64 {
        self.written + self.staged.len() as u64
    }

    /// Write what is staged into the buffer (ordered before the next submit).
    fn write_staged(&mut self, queue: &wgpu::Queue) {
        if let Some(buffer) = &self.buffer
            && !self.staged.is_empty()
        {
            queue.write_buffer(buffer, self.written, &self.staged);
            self.written += self.staged.len() as u64;
            self.staged.clear();
        }
    }

    /// Stage `bytes`, zero-padded to `size`, at an offset aligned to `align`.
    fn stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], size: u64, align: u64) -> Slice {
        let size = size.max(bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(4);
        let mut offset = self.cursor().next_multiple_of(align);
        let room = self.buffer.as_ref().map_or(0, wgpu::Buffer::size);
        if offset + size > room {
            // Earlier slices of the old buffer stay valid: what they hold is written, and bind groups keep it alive.
            self.write_staged(queue);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("uploads"), size: CHUNK.max(size), usage: USAGE, mapped_at_creation: false }));
            self.written = 0;
            offset = 0;
        }
        let at = (offset - self.written) as usize;
        self.staged.resize(at, 0);
        self.staged.extend_from_slice(bytes);
        self.staged.resize(at + size as usize, 0);
        Slice { buffer: self.buffer.clone().expect("chunk"), offset, size }
    }
}

#[derive(Default)]
struct State {
    chunks: HashMap<ThreadId, Chunk>,
    /// Ordered buffer writes (copies out of the upload buffers) since the last queued command buffer.
    copies: Option<wgpu::CommandEncoder>,
    pending: Vec<wgpu::CommandBuffer>,
}

impl State {
    fn chunk(&mut self) -> &mut Chunk {
        self.chunks.entry(std::thread::current().id()).or_insert_with(|| Chunk { buffer: None, staged: Vec::new(), written: 0, owner: ALIVE.with(Arc::downgrade) })
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
    /// Stage `bytes`, zero-padded to `size`, at an offset aligned to `align`, in this thread's upload buffer.
    pub fn upload(&self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], size: u64, align: u64) -> Slice {
        self.state.lock().unwrap().chunk().stage(device, queue, bytes, size, align)
    }

    /// Stage `bytes` and record their copy into `dst` at `offset`, ordered with the queued command buffers.
    pub fn write(&self, device: &wgpu::Device, queue: &wgpu::Queue, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let mut s = self.state.lock().unwrap();
        let src = s.chunk().stage(device, queue, bytes, 0, wgpu::COPY_BUFFER_ALIGNMENT);
        let e = s.copies.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("buffer writes") }));
        e.copy_buffer_to_buffer(&src.buffer, src.offset, dst, offset, src.size);
    }

    pub fn push(&self, commands: wgpu::CommandBuffer) {
        let mut s = self.state.lock().unwrap();
        s.close_copies();
        s.pending.push(commands);
    }

    /// Write every thread's staged uploads and hand the queued command buffers, in order, to
    /// `submit`. With `end_frame`, this thread's later uploads reuse its buffer from the start:
    /// everything that read it is queued.
    /// Gotcha: `submit` runs under the lock, or two threads' flushes could submit out of order.
    pub fn drain(&self, queue: &wgpu::Queue, end_frame: bool, submit: impl FnOnce(Vec<wgpu::CommandBuffer>)) {
        let mut s = self.state.lock().unwrap();
        for c in s.chunks.values_mut() {
            c.write_staged(queue);
        }
        s.chunks.retain(|_, c| c.owner.strong_count() > 0);
        s.close_copies();
        if end_frame {
            s.chunk().written = 0;
        }
        submit(std::mem::take(&mut s.pending));
    }
}
