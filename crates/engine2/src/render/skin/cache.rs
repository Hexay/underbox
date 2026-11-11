//! The animated vertex cache: one storage buffer of skinned vertices (24 bytes each, see
//! `vs_decompress.fxc`), bump-allocated per frame. Growing keeps what earlier dispatches this
//! frame wrote, so a handle managed holds stays valid all frame.

use crate::render::{buffers, device};
use std::sync::{Arc, Mutex};

pub const VERTEX_BYTES: u64 = 24;
const MIN_VERTICES: u64 = 4096;

struct Cache {
    handle: buffers::Handle,
    buffer: Arc<render::Buffer>,
    used: u64,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn create(gpu: &render::Gpu, vertices: u64) -> Arc<render::Buffer> {
    Arc::new(render::Buffer::new(gpu, "vertex cache", render::BufferKind::Storage, vertices.max(MIN_VERTICES) * VERTEX_BYTES))
}

fn with<R>(f: impl FnOnce(&render::Gpu, &mut Cache) -> R) -> Option<R> {
    let gpu = device::gpu()?;
    let mut cache = CACHE.lock().unwrap();
    let c = cache.get_or_insert_with(|| {
        let buffer = create(gpu, MIN_VERTICES);
        Cache { handle: buffers::insert(buffer.clone()), buffer, used: 0 }
    });
    Some(f(gpu, c))
}

/// The first vertex of `n` free ones this frame; `None` without a device.
pub fn allocate(n: u32) -> Option<u32> {
    with(|gpu, c| {
        let first = c.used;
        c.used += u64::from(n);
        if c.used * VERTEX_BYTES > c.buffer.size() {
            let grown = create(gpu, c.used.next_power_of_two());
            c.buffer.copy_to(gpu, &grown, 0, 0, c.buffer.size());
            buffers::replace(c.handle, grown.clone());
            c.buffer = grown;
        }
        first as u32
    })
}

/// The cache's handle (`GetVertexCacheBuffer`); the null handle without a device.
pub fn handle() -> buffers::Handle {
    with(|_, c| c.handle).unwrap_or_default()
}

pub fn buffer() -> Option<Arc<render::Buffer>> {
    with(|_, c| c.buffer.clone())
}

/// A new frame: allocations start over.
pub fn begin_frame() {
    if let Some(c) = CACHE.lock().unwrap().as_mut() {
        c.used = 0;
    }
}
