//! GPU copies of `MeshGlue` vertex and index buffers, keyed by buffer identity and re-uploaded
//! when the buffer's version moves (a lock/unlock, `Set*BufferData` or resize). Entries for
//! buffers that are gone are dropped by [`prune`] once per frame.

use crate::mesh::buffers::{Buffer, IndexBuffer, VertexBuffer};
use crate::mesh::layout::{Layout, format};
use crate::render::device;
use render::wgpu::{IndexFormat, VertexFormat};
use render::{BufferKind, Placeholder, Semantic, VertexBufferKey};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

struct Entry {
    alive: Box<dyn Fn() -> bool + Send>,
    version: u64,
    gpu: Arc<render::Buffer>,
}

static CACHE: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(Default::default);

fn upload<M: Send + Sync + 'static>(b: &Arc<Buffer<M>>, kind: BufferKind) -> Option<Arc<render::Buffer>> {
    let gpu = device::gpu()?;
    let key = Arc::as_ptr(b) as usize;
    let version = b.version();
    let mut cache = CACHE.lock().unwrap();
    if let Some(e) = cache.get(&key)
        && e.version == version
        && (e.alive)()
    {
        return Some(e.gpu.clone());
    }
    let bytes = b.read();
    if bytes.is_empty() {
        return None;
    }
    let buffer = match cache.get(&key) {
        Some(e) if (e.alive)() && e.gpu.size() >= bytes.len() as u64 => {
            e.gpu.write(gpu, 0, &bytes);
            e.gpu.clone()
        }
        _ => Arc::new(render::Buffer::with_data(gpu, "mesh", kind, &bytes)),
    };
    let weak: Weak<Buffer<M>> = Arc::downgrade(b);
    cache.insert(key, Entry { alive: Box::new(move || weak.strong_count() > 0), version, gpu: buffer.clone() });
    Some(buffer)
}

pub fn vertices(b: &Arc<VertexBuffer>) -> Option<Arc<render::Buffer>> {
    upload(b, BufferKind::Vertex)
}

pub fn indices(b: &Arc<IndexBuffer>) -> Option<(Arc<render::Buffer>, IndexFormat)> {
    let format = if b.meta { IndexFormat::Uint32 } else { IndexFormat::Uint16 };
    upload(b, BufferKind::Index).map(|g| (g, format))
}

/// Forget GPU copies whose mesh buffer was released.
pub fn prune() {
    CACHE.lock().unwrap().retain(|_, e| (e.alive)());
}

fn vertex_format(f: u32) -> Option<VertexFormat> {
    Some(match f {
        format::RGBA32F => VertexFormat::Float32x4,
        format::RGB32F => VertexFormat::Float32x3,
        format::RG32F => VertexFormat::Float32x2,
        format::R32F => VertexFormat::Float32,
        format::RGBA8_UNORM => VertexFormat::Unorm8x4,
        _ => return None,
    })
}

/// The placeholder's vertex buffers for a mesh layout; `None` without a float position.
pub fn vertex_keys(layout: &Layout) -> Option<Vec<VertexBufferKey>> {
    let mut attrs = Vec::new();
    for f in &layout.fields {
        let semantic = match (f.semantic.as_str(), f.index) {
            ("position", 0) => Semantic::Position,
            ("normal", 0) => Semantic::Normal,
            ("color", 0) => Semantic::Color,
            _ => continue,
        };
        // Gotcha: wgpu needs 4-byte aligned attribute offsets; such attributes are skipped.
        let Some(fmt) = vertex_format(f.format).filter(|_| f.offset % 4 == 0) else { continue };
        if !attrs.iter().any(|(s, _, _)| *s == semantic) {
            attrs.push((semantic, fmt, f.offset as u64));
        }
    }
    let stride = layout.stride as u64;
    let aligned = stride > 0 && stride.is_multiple_of(4);
    (aligned && attrs.iter().any(|a| a.0 == Semantic::Position)).then(|| Placeholder::vertex_keys(stride, &attrs))
}
