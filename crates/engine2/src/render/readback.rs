//! Reading textures and GPU buffers back: `g_pRenderDevice.ReadTexturePixels` (synchronous, into
//! managed memory, converting the format) and the asynchronous context reads, which hand the
//! bytes to managed's `AsyncGPUReadback` through a callback object.
//!
//! Gotcha: contexts execute eagerly (context.rs), so an async read completes inside the call and
//! dispatches before returning. The bytes live in the callback object until managed calls `Done`
//! (it reads them on a worker thread), which also deletes the object when asked to.

use crate::render::{buffers, device, texture};
use abi::callbacks::sandbox_async_gpu_readback as managed;
use abi::slots::{c_read_buffer_managed_callback as rbc, c_read_texture_pixels_managed_callback as rtc};
use abi::slots::{g_p_render_device as dev, i_render_context as ctx};
use abi::structs::NativeRect;
use abi::{NativeRef, RawPtr, Registry, bind};
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// `ImageFormat` RGBA8888 and BGRA8888, the formats a swizzle converts between.
const RGBA8888: i32 = 0;
const BGRA8888: i32 = 12;

/// The pixels of `[x, y, w, h]` (empty = the whole mip) of a texture's `mip`, tightly packed, with
/// their `ImageFormat`: from the GPU texture when there is one, else from the CPU copy (mip 0).
pub fn read(t: texture::Ref, rect: NativeRect, mip: i32) -> Option<(render::Frame, i32)> {
    let tex = texture::resolve(t)?;
    let format = tex.image_format();
    let mip = u32::try_from(mip).ok()?;
    if let (Some(gpu), Some(g)) = (device::gpu(), tex.gpu.as_ref()) {
        let (mw, mh) = g.mip_size(mip);
        let r = if rect.w <= 0 || rect.h <= 0 { [0, 0, mw, mh] } else { [rect.x.max(0) as u32, rect.y.max(0) as u32, rect.w as u32, rect.h as u32] };
        return g.read_region(gpu, mip, r).map(|f| (f, format));
    }
    let full = tex.cpu_mip0().filter(|_| mip == 0)?;
    let (w, h, texel) = (full.width, full.height, full.texel_size);
    if rect.w <= 0 || rect.h <= 0 {
        return Some((full, format));
    }
    let (x, y, rw, rh) = (rect.x.max(0) as u32, rect.y.max(0) as u32, rect.w as u32, rect.h as u32);
    if x + rw > w || y + rh > h {
        return None;
    }
    let mut out = render::Frame::filled(rw, rh, &vec![0; texel as usize]);
    for row in 0..rh {
        for col in 0..rw {
            out.set_texel(col, row, full.texel(x + col, y + row));
        }
    }
    Some((out, format))
}

/// Pixels in `from` converted to `to`: identical formats copy, RGBA/BGRA swizzle, others fail.
fn convert(frame: &render::Frame, from: i32, to: i32) -> Option<Vec<u8>> {
    match (from, to) {
        _ if from == to => Some(frame.pixels.clone()),
        (RGBA8888, BGRA8888) | (BGRA8888, RGBA8888) => Some(frame.pixels.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn read_pixels(t: texture::Ref, src: &mut NativeRect, _slice: i32, mip: i32, dst: &mut NativeRect, data: RawPtr<c_void>, dst_format: i64, stride: i32) -> bool {
    let Some((frame, format)) = read(t, *src, mip) else { return false };
    let Some(bytes) = convert(&frame, format, dst_format as i32) else { return false };
    if data.is_null() {
        return false;
    }
    let row = (frame.width * frame.texel_size) as usize;
    let stride = if stride > 0 { stride as usize } else { (dst.x.max(0) as usize + frame.width as usize) * frame.texel_size as usize };
    let (dx, dy) = (dst.x.max(0) as usize * frame.texel_size as usize, dst.y.max(0) as usize);
    let len = (dy + frame.height as usize - 1) * stride + dx + row;
    // SAFETY: managed passes a pinned buffer covering dstRect at nDstStride (Texture.Read.cs).
    let out = unsafe { data.cast::<u8>().as_mut_slice(len) };
    for (y, src_row) in bytes.chunks(row).enumerate() {
        let at = (dy + y) * stride + dx;
        out[at..at + row].copy_from_slice(src_row);
    }
    true
}

/// The native side of a managed read callback: its id, and the bytes handed out until `Done`.
#[derive(Default)]
pub struct Pending {
    id: AtomicI32,
    data: Mutex<Vec<u8>>,
    delete_when_done: AtomicBool,
}

fn done<C: abi::NativeClass>(r: NativeRef<C>) {
    let Some(p) = objects::get::<_, Pending>(r) else { return };
    std::mem::take(&mut *p.data.lock().unwrap());
    if p.delete_when_done.load(Ordering::Acquire) {
        objects::destroy(r);
    }
}

/// Keep `bytes` in the callback object, returning their address for managed.
fn hold<C>(r: NativeRef<C>, bytes: Vec<u8>, delete: bool) -> Option<RawPtr<c_void>> {
    let p = objects::get::<_, Pending>(r)?;
    p.delete_when_done.store(delete, Ordering::Release);
    let mut data = p.data.lock().unwrap();
    *data = bytes;
    Some(RawPtr::from_raw(data.as_mut_ptr().cast()))
}

fn read_async(_ctx: NativeRef<abi::class::IRenderContext>, t: texture::Ref, cb: NativeRef<abi::class::IReadTexturePixelsCallback>, rect: NativeRect, _slice: i32, mip: i32, delete: bool) {
    let caller: NativeRef<abi::class::CReadTexturePixelsManagedCallback> = cb.cast();
    let (frame, format) = read(t, rect, mip).unwrap_or_else(|| (render::Frame::filled(0, 0, &[0; 4]), RGBA8888));
    let pitch = (frame.width * frame.texel_size) as i32;
    let (w, h) = (frame.width as i32, frame.height as i32);
    let Some(ptr) = hold(caller, frame.pixels, delete) else { return };
    if abi::rt::callbacks::is_set(0) {
        managed::dispatch_managed_read_texture_callback(caller, ptr, format.into(), mip, w, h, pitch);
    }
}

fn read_buffer_async(_ctx: NativeRef<abi::class::IRenderContext>, b: buffers::Handle, cb: NativeRef<abi::class::IReadBufferCallback>, offset: i32, len: i32, delete: bool) {
    let caller: NativeRef<abi::class::CReadBufferManagedCallback> = cb.cast();
    let (Ok(offset), Ok(len)) = (u64::try_from(offset), u64::try_from(len)) else { return };
    let bytes = match (device::gpu(), buffers::get(b)) {
        (Some(gpu), Some(buf)) if offset + len <= buf.size() => buf.read(gpu, offset, len),
        _ => Vec::new(),
    };
    let n = bytes.len() as i32;
    let Some(ptr) = hold(caller, bytes, delete) else { return };
    if abi::rt::callbacks::is_set(1) {
        managed::dispatch_managed_read_buffer_callback(caller, ptr, n);
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, dev::READ_TEXTURE_PIXELS, read_pixels);
    bind!(r, ctx::READ_TEXTURE_PIXELS, read_async);
    bind!(r, ctx::READ_BUFFER, read_buffer_async);
    bind!(r, rtc::CREATE, || objects::create(Pending::default()));
    bind!(r, rtc::DELETE_THIS, |c| {
        objects::destroy(c);
    });
    bind!(r, rtc::SET_MANAGED_ID, |c, id| objects::get::<_, Pending>(c).iter().for_each(|p| p.id.store(id, Ordering::Relaxed)));
    bind!(r, rtc::GET_MANAGED_ID, |c| objects::get::<_, Pending>(c).map_or(0, |p| p.id.load(Ordering::Relaxed)));
    bind!(r, rtc::DONE, done);
    bind!(r, rbc::CREATE, || objects::create(Pending::default()));
    bind!(r, rbc::DELETE_THIS, |c| {
        objects::destroy(c);
    });
    bind!(r, rbc::SET_MANAGED_ID, |c, id| objects::get::<_, Pending>(c).iter().for_each(|p| p.id.store(id, Ordering::Relaxed)));
    bind!(r, rbc::GET_MANAGED_ID, |c| objects::get::<_, Pending>(c).map_or(0, |p| p.id.load(Ordering::Relaxed)));
    bind!(r, rbc::DONE, done);
}

#[cfg(test)]
#[path = "readback_tests.rs"]
mod tests;
