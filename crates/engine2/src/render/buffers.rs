//! GPU buffers (`RenderBufferHandle_t`, managed's `GpuBuffer`). Without a device there are none:
//! creation answers the null handle, as the empty device does.

use crate::render::device;
use abi::slots::{g_p_render_device as dev, render_tools as tools};
use abi::structs::BufferDesc;
use abi::{PtrHandle, RawPtr, Registry, bind, handle};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

pub type Handle = PtrHandle<handle::RenderBufferHandle_t>;

static BUFFERS: LazyLock<RwLock<HashMap<usize, Arc<render::Buffer>>>> = LazyLock::new(Default::default);
static NEXT: AtomicUsize = AtomicUsize::new(1);

pub fn get(h: Handle) -> Option<Arc<render::Buffer>> {
    BUFFERS.read().unwrap().get(&h.value).cloned()
}

pub(crate) fn create(_kind: i64, desc: BufferDesc, _usage: i64, name: Option<&CStr>) -> Handle {
    let Some(gpu) = device::gpu() else { return Handle::default() };
    let size = i64::from(desc.m_n_element_count.max(0)) * i64::from(desc.m_n_element_size_in_bytes.max(0));
    let label = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let buffer = render::Buffer::new(gpu, &label, render::BufferKind::Storage, size as u64);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    BUFFERS.write().unwrap().insert(id, Arc::new(buffer));
    Handle::new(id)
}

pub(crate) fn destroy(h: Handle) {
    BUFFERS.write().unwrap().remove(&h.value);
}

fn in_range(b: &render::Buffer, offset: u32, len: u32) -> bool {
    u64::from(offset) + u64::from(len) <= b.size()
}

/// Synchronous read into managed memory.
pub(crate) fn read(h: Handle, offset: u32, dst: RawPtr<c_void>, len: u32) -> bool {
    let (Some(gpu), Some(b)) = (device::gpu(), get(h)) else { return false };
    if dst.is_null() || !in_range(&b, offset, len) {
        return false;
    }
    let bytes = b.read(gpu, offset.into(), len.into());
    // SAFETY: managed passes a pinned buffer of nBytesToRead bytes (GpuBuffer.GetData).
    unsafe { dst.cast::<u8>().as_mut_slice(len as usize) }.copy_from_slice(&bytes);
    true
}

/// Upload managed bytes at a byte offset. Context writes execute eagerly (see context.rs).
pub(crate) fn set_data(_ctx: super::context::Ref, h: Handle, data: RawPtr<c_void>, len: u32, offset: u32) {
    let (Some(gpu), Some(b)) = (device::gpu(), get(h)) else { return };
    if !in_range(&b, offset, len) {
        return;
    }
    // SAFETY: managed passes a pinned span of nDataSize bytes (GpuBuffer.SetDataInternal).
    let bytes = unsafe { data.cast::<u8>().as_slice(len as usize) };
    b.write(gpu, offset.into(), bytes);
}

fn fill(_ctx: super::context::Ref, h: Handle, value: u32) {
    if let (Some(gpu), Some(b)) = (device::gpu(), get(h)) {
        b.fill(gpu, value);
    }
}

pub(crate) fn copy(_ctx: super::context::Ref, src: Handle, dst: Handle, src_offset: u32, dst_offset: u32, len: u32) {
    let (Some(gpu), Some(s), Some(d)) = (device::gpu(), get(src), get(dst)) else { return };
    let aligned = [src_offset, dst_offset, len].iter().all(|v| v % 4 == 0);
    if aligned && in_range(&s, src_offset, len) && in_range(&d, dst_offset, len) {
        s.copy_to(gpu, &d, src_offset.into(), dst_offset.into(), len.into());
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, dev::CREATE_GPU_BUFFER, create);
    bind!(r, dev::DESTROY_GPU_BUFFER, destroy);
    bind!(r, dev::READ_BUFFER, read);
    bind!(r, tools::SET_GPU_BUFFER_DATA, set_data);
    bind!(r, tools::FILL_GPU_BUFFER, fill);
    bind!(r, tools::COPY_GPU_BUFFER, copy);
}
