//! What managed binds for draws besides vertices and targets: constant buffers (stored on the
//! render attributes by block name, which the material binder prefers over its own fill),
//! textures by slot, and raw shader handles.

use crate::render::draw::immediate;
use crate::render::{attributes, context, texture};
use abi::slots::{i_render_context as ctx, render_tools as tools};
use abi::{RawPtr, Registry, StringToken, bind};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{LazyLock, Mutex};

/// Allocations managed asked for with `CreateConstantBuffer`, by address, with their layouts.
static CONSTANT_BUFFERS: LazyLock<Mutex<HashMap<usize, std::alloc::Layout>>> = LazyLock::new(Default::default);

fn create_constant_buffer(size: i32) -> RawPtr<c_void> {
    let Ok(layout) = std::alloc::Layout::from_size_align(size.max(1) as usize, 16) else { return RawPtr::NULL };
    // SAFETY: layout has a nonzero size.
    let p = unsafe { std::alloc::alloc_zeroed(layout) };
    if !p.is_null() {
        CONSTANT_BUFFERS.lock().unwrap().insert(p as usize, layout);
    }
    RawPtr::from_raw(p.cast())
}

fn destroy_constant_buffer(p: RawPtr<c_void>) {
    if let Some(layout) = CONSTANT_BUFFERS.lock().unwrap().remove(&(p.as_ptr() as usize)) {
        // SAFETY: allocated by create_constant_buffer with this layout.
        unsafe { std::alloc::dealloc(p.as_ptr().cast(), layout) };
    }
}

/// `size` bytes managed passed, or nothing for a null or empty block.
fn bytes<'a>(data: RawPtr<c_void>, size: i32) -> Option<&'a [u8]> {
    // SAFETY: managed passes `size` readable bytes (RenderAttributes.SetData, ShadowMapper).
    (!data.is_null() && size > 0).then(|| unsafe { data.cast::<u8>().as_slice(size as usize) })
}

/// Block `name`'s data for later draws with `attrs`, and into `buffer` when it is one of ours.
fn set_constant_buffer(attrs: attributes::Ref, name: StringToken, buffer: RawPtr<c_void>, data: RawPtr<c_void>, size: i32) {
    let Some(src) = bytes(data, size) else { return };
    attributes::set_constant_buffer(attrs, name, src);
    if let Some(layout) = CONSTANT_BUFFERS.lock().unwrap().get(&(buffer.as_ptr() as usize)) {
        let n = src.len().min(layout.size());
        // SAFETY: buffer is a live create_constant_buffer allocation of layout.size() bytes.
        unsafe { buffer.cast::<u8>().as_ptr().copy_from_nonoverlapping(src.as_ptr(), n) };
    }
}

/// `IRenderContext.BindTexture`: the texture at `slot` for this context's later draws.
fn bind_texture(c: context::Ref, slot: i32, t: texture::Ref) {
    let Ok(slot) = u32::try_from(slot) else { return };
    immediate::with(c, |s| {
        s.slots.retain(|(n, _)| *n != slot);
        if !t.is_null() {
            s.slots.push((slot, t));
        }
    });
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::CREATE_CONSTANT_BUFFER, create_constant_buffer);
    bind!(r, tools::DESTROY_CONSTANT_BUFFER, destroy_constant_buffer);
    bind!(r, tools::SET_CONSTANT_BUFFER_DATA, |_, attrs, name, buffer, data, size| set_constant_buffer(attrs, name, buffer, data, size));
    bind!(r, tools::SET_DYNAMIC_CONSTANT_BUFFER_DATA, |attrs, name, _, data, size| set_constant_buffer(attrs, name, RawPtr::NULL, data, size));
    bind!(r, ctx::BIND_TEXTURE, bind_texture);
    // TODO: slot-addressed blocks and raw shader handles pair with BindVertexShader/BindPixelShader,
    // whose RenderShaderHandle_t nothing in igen_engine produces; accepted and dropped.
    // TODO: binds by register slot, not name; nothing in sbox-public calls it.
    bind!(r, ctx::SET_DYNAMIC_CONSTANT_BUFFER_DATA, |_, _, _, _, _| {});
    bind!(r, ctx::BIND_VERTEX_SHADER, |_, _, _| {});
    bind!(r, ctx::BIND_PIXEL_SHADER, |_, _| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_buffer_data_lands_on_the_attributes_and_the_buffer() {
        let attrs = attributes::create();
        let buffer = create_constant_buffer(8);
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        set_constant_buffer(attrs, StringToken::of("MyCB"), buffer, RawPtr::from_raw(data.as_ptr() as *mut c_void), data.len() as i32);
        assert_eq!(attributes::constant_buffer(attrs, "MyCB").as_deref(), Some(&data[..]));
        // SAFETY: an 8-byte block from create_constant_buffer.
        assert_eq!(unsafe { buffer.cast::<u8>().as_slice(8) }, &data[..8]);
        destroy_constant_buffer(buffer);
        objects::destroy(attrs);
    }
}
