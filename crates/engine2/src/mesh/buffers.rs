//! CPU-side vertex and index buffers (`VertexBufferHandle_t`, `IndexBufferHandle_t`). Managed
//! never frees one: the [`RenderMesh`](super::render_mesh::RenderMesh) it is bound to releases it.
//! The renderer uploads from [`Buffer::read`], re-uploading when [`Buffer::version`] moves.
//!
//! Gotcha: `MeshGlue`'s `nDataSize` means elements for creation and resizing, bytes for
//! lock/unlock and `Set*BufferData` (Mesh.VertexBuffer.cs, Mesh.IndexBuffer.cs).

use super::layout::{self, Layout, VertexField};
use abi::slots::mesh_glue as g;
use abi::{PtrHandle, RawPtr, Registry, bind, handle};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock};

pub type VertexHandle = PtrHandle<handle::VertexBufferHandle_t>;
pub type IndexHandle = PtrHandle<handle::IndexBufferHandle_t>;

/// A buffer's bytes plus what never changes about them: a vertex layout, or index width.
pub struct Buffer<M> {
    id: usize,
    pub meta: M,
    bytes: Mutex<Vec<u8>>,
    version: AtomicU64,
}

pub type VertexBuffer = Buffer<Arc<Layout>>;
/// `meta` is true for 32-bit indices.
pub type IndexBuffer = Buffer<bool>;

impl<M> Buffer<M> {
    pub fn new(meta: M, bytes: Vec<u8>) -> Self {
        Self { id: 0, meta, bytes: Mutex::new(bytes), version: AtomicU64::new(0) }
    }

    pub fn read(&self) -> MutexGuard<'_, Vec<u8>> {
        self.bytes.lock().unwrap()
    }

    /// Bumped on every write, so an uploader knows its copy is stale.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn id(&self) -> usize {
        self.id
    }

    fn touch(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    fn write(&self, offset: usize, src: &[u8]) {
        let mut bytes = self.read();
        if let Some(dst) = bytes.get_mut(offset..offset + src.len()) {
            dst.copy_from_slice(src);
        }
        drop(bytes);
        self.touch();
    }
}

impl VertexBuffer {
    pub fn stride(&self) -> usize {
        self.meta.stride
    }

    pub fn count(&self) -> usize {
        if self.stride() == 0 { 0 } else { self.read().len() / self.stride() }
    }
}

impl IndexBuffer {
    pub fn indices(&self) -> Vec<u32> {
        let bytes = self.read();
        if self.meta {
            bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
        } else {
            bytes.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes(c.try_into().unwrap()))).collect()
        }
    }

    fn width(&self) -> usize {
        if self.meta { 4 } else { 2 }
    }
}

struct Table<M>(LazyLock<RwLock<HashMap<usize, Arc<Buffer<M>>>>>);

static NEXT: AtomicUsize = AtomicUsize::new(1);
static VERTICES: Table<Arc<Layout>> = Table(LazyLock::new(Default::default));
static INDICES: Table<bool> = Table(LazyLock::new(Default::default));

impl<M> Table<M> {
    fn insert(&self, mut b: Buffer<M>) -> usize {
        b.id = NEXT.fetch_add(1, Ordering::Relaxed);
        let id = b.id;
        self.0.write().unwrap().insert(id, Arc::new(b));
        id
    }

    fn get(&self, id: usize) -> Option<Arc<Buffer<M>>> {
        self.0.read().unwrap().get(&id).cloned()
    }

    fn remove(&self, id: usize) {
        self.0.write().unwrap().remove(&id);
    }
}

pub fn vertex_buffer(h: VertexHandle) -> Option<Arc<VertexBuffer>> {
    VERTICES.get(h.value)
}

pub fn index_buffer(h: IndexHandle) -> Option<Arc<IndexBuffer>> {
    INDICES.get(h.value)
}

/// Drop a buffer's handle (its mesh is gone).
pub(super) fn release_vertex(id: usize) {
    VERTICES.remove(id);
}

pub(super) fn release_index(id: usize) {
    INDICES.remove(id);
}

/// `len` bytes managed passed, or empty for a null pointer.
fn bytes_at(data: RawPtr<c_void>, len: usize) -> Vec<u8> {
    // SAFETY: managed passes a pinned span of at least `len` bytes, or null (Mesh.VertexBuffer.cs).
    unsafe { data.cast::<u8>().as_slice(len) }.to_vec()
}

/// A zeroed buffer of `count` elements of `size` bytes, its prefix filled from `data`.
fn filled(count: i32, size: usize, data: RawPtr<c_void>, data_elements: i32) -> Vec<u8> {
    let count = count.max(0) as usize;
    let mut bytes = vec![0; count * size];
    let given = data_elements.clamp(0, count as i32) as usize * size;
    bytes[..given].copy_from_slice(&bytes_at(data, given));
    bytes
}

pub fn create_vertices(count: i32, layout: Arc<Layout>, data: RawPtr<c_void>, data_elements: i32) -> VertexHandle {
    let bytes = filled(count, layout.stride, data, data_elements);
    VertexHandle::new(VERTICES.insert(Buffer::new(layout, bytes)))
}

pub fn create_indices(count: i32, wide: bool, data: RawPtr<c_void>, data_elements: i32) -> IndexHandle {
    let bytes = filled(count, if wide { 4 } else { 2 }, data, data_elements);
    IndexHandle::new(INDICES.insert(Buffer::new(wide, bytes)))
}

fn legacy_layout(stride: i32, names: Option<&CStr>, fields: RawPtr<c_void>, n: i32) -> Arc<Layout> {
    // SAFETY: managed passes a pinned VertexField[n] (at most 16).
    let fields = unsafe { fields.cast::<VertexField>().as_slice(n.clamp(0, 16) as usize) };
    let names = names.map(CStr::to_string_lossy).unwrap_or_default();
    Arc::new(layout::from_fields(stride.max(0) as usize, &names, fields))
}

/// The locked region's address (managed writes straight into it), or null when out of range.
fn lock<M>(b: Option<Arc<Buffer<M>>>, size: i32, offset: i32) -> RawPtr<c_void> {
    let (Some(b), Ok(size), Ok(offset)) = (b, usize::try_from(size), usize::try_from(offset)) else { return RawPtr::NULL };
    let mut bytes = b.read();
    if offset + size > bytes.len() {
        return RawPtr::NULL;
    }
    // Gotcha: the Vec isn't resized between Lock and Unlock, so the address stays valid unguarded.
    RawPtr::from_raw(bytes[offset..].as_mut_ptr().cast())
}

fn set_data<M>(b: Option<Arc<Buffer<M>>>, data: RawPtr<c_void>, size: i32, offset: i32) {
    let (Some(b), Ok(size), Ok(offset)) = (b, usize::try_from(size), usize::try_from(offset)) else { return };
    b.write(offset, &bytes_at(data, size));
}

/// Grow or shrink to `count` elements keeping the prefix; the handle stays the same.
fn resize<M>(b: &Buffer<M>, count: i32, size: usize) {
    b.read().resize(count.max(0) as usize * size, 0);
    b.touch();
}

pub fn register(r: &mut Registry) {
    bind!(r, g::CREATE_VERTEX_BUFFER, |stride, count, names, fields, n, data, data_elements| create_vertices(count, legacy_layout(stride, names, fields, n), data, data_elements));
    bind!(r, g::CREATE_VERTEX_BUFFER_1, |count, l, data| {
        let Some(layout) = layout::get(l) else { return VertexHandle::default() };
        let full = if data.is_null() { 0 } else { count };
        create_vertices(count, layout, data, full)
    });
    bind!(r, g::CREATE_INDEX_BUFFER, create_indices);
    bind!(r, g::LOCK_VERTEX_BUFFER, |h, size, offset| lock(vertex_buffer(h), size, offset));
    bind!(r, g::LOCK_INDEX_BUFFER, |h, size, offset| lock(index_buffer(h), size, offset));
    bind!(r, g::UNLOCK_VERTEX_BUFFER, |h, _, _, _| vertex_buffer(h).iter().for_each(|b| b.touch()));
    bind!(r, g::UNLOCK_INDEX_BUFFER, |h, _, _, _| index_buffer(h).iter().for_each(|b| b.touch()));
    bind!(r, g::SET_VERTEX_BUFFER_DATA, |h, data, size, offset| set_data(vertex_buffer(h), data, size, offset));
    bind!(r, g::SET_INDEX_BUFFER_DATA, |h, data, size, offset| set_data(index_buffer(h), data, size, offset));
    bind!(r, g::SET_VERTEX_BUFFER_SIZE, |h, count| {
        vertex_buffer(h).inspect(|b| resize(b, count, b.stride())).map_or(VertexHandle::default(), |_| h)
    });
    bind!(r, g::SET_INDEX_BUFFER_SIZE, |h, count| index_buffer(h).inspect(|b| resize(b, count, b.width())).map_or(IndexHandle::default(), |_| h));
}
