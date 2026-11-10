//! `IRenderContext`: bound targets, viewport and attributes for a stream of render commands.
//!
//! Gotcha: commands execute eagerly on the device's queue rather than into a display list, so
//! `Submit` only orders against later reads; the order managed issues them in is kept.

use crate::render::draw::Target;
use crate::render::draw::immediate::DrawState;
use crate::render::{attributes, device, texture};
use abi::slots::{g_p_render_device as dev, i_render_context as ctx, render_tools as tools};
use abi::structs::{NativeRect, RenderViewport, Vector4};
use abi::{NativeRef, Registry, bind, class};
use std::sync::Mutex;

pub type Ref = NativeRef<class::IRenderContext>;

#[derive(Default)]
struct Bindings {
    color: Option<texture::Ref>,
    depth: Option<texture::Ref>,
    viewport: RenderViewport,
    scissor: Option<NativeRect>,
}

pub struct RenderContext {
    pub attributes: attributes::Ref,
    bindings: Mutex<Bindings>,
    pub draw: Mutex<DrawState>,
}

pub fn create(_flags: u32) -> Ref {
    objects::create(RenderContext { attributes: attributes::create(), bindings: Mutex::default(), draw: Mutex::default() })
}

pub fn get(r: Ref) -> Option<std::sync::Arc<RenderContext>> {
    objects::get::<_, RenderContext>(r)
}

/// The bound targets, viewport and scissor draws go to.
pub fn target(r: Ref) -> Target {
    with(r, |b| Target { color: b.color.unwrap_or_default(), depth: b.depth.unwrap_or_default(), viewport: b.viewport, scissor: b.scissor })
}

/// Bind `t`'s targets, viewport and scissor at once.
pub fn bind_target(r: Ref, t: &Target) {
    bind_targets(r, t.color, t.depth);
    with(r, |b| {
        if t.viewport.m_n_width > 0 && t.viewport.m_n_height > 0 {
            b.viewport = t.viewport;
        }
        b.scissor = t.scissor;
    });
}

pub fn release(r: Ref) {
    if let Some(c) = objects::get::<_, RenderContext>(r) {
        objects::destroy(c.attributes);
    }
    objects::destroy(r);
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut Bindings) -> R) -> R {
    objects::get::<_, RenderContext>(r).map(|c| f(&mut c.bindings.lock().unwrap())).unwrap_or_default()
}

fn some(t: texture::Ref) -> Option<texture::Ref> {
    (!t.is_null()).then_some(t)
}

/// Bind colour and depth targets; the viewport resets to cover the colour (or depth) target.
pub fn bind_targets(r: Ref, color: texture::Ref, depth: texture::Ref) {
    let size = some(color).or(some(depth)).and_then(texture::resolve).and_then(|t| t.gpu.as_ref().map(|g| (g.desc.width, g.desc.height)));
    with(r, |b| {
        (b.color, b.depth, b.scissor) = (some(color), some(depth), None);
        if let Some((w, h)) = size {
            b.viewport = RenderViewport { m_n_width: w as i32, m_n_height: h as i32, max_z: 1.0, ..Default::default() };
        }
    });
}

/// TODO: clears cover the whole target; a viewport or scissor smaller than it needs a quad draw.
fn clear(r: Ref, color: Vector4, clear_color: bool, clear_depth: bool, _clear_stencil: bool) {
    let Some(gpu) = device::gpu() else { return };
    let (target, depth) = with(r, |b| (b.color, b.depth));
    let v = color.vec;
    let rgba = render::wgpu::Color { r: v.x.into(), g: v.y.into(), b: v.z.into(), a: v.w.into() };
    let gpu_of = |t: Option<texture::Ref>| t.and_then(texture::resolve);
    if clear_color && let Some(t) = gpu_of(target) && let Some(tex) = &t.gpu {
        tex.clear(gpu, rgba, 1.0);
    }
    if clear_depth && let Some(t) = gpu_of(depth) && let Some(tex) = &t.gpu {
        tex.clear(gpu, rgba, 1.0);
    }
}

fn set_viewport(r: Ref, x: i32, y: i32, w: i32, h: i32) {
    with(r, |b| b.viewport = RenderViewport { m_n_top_left_x: x, m_n_top_left_y: y, m_n_width: w, m_n_height: h, max_z: 1.0, ..b.viewport });
}

pub fn register(r: &mut Registry) {
    bind!(r, dev::CREATE_RENDER_CONTEXT, create);
    bind!(r, dev::RELEASE_RENDER_CONTEXT, release);
    bind!(r, ctx::GET_ATTRIBUTES_PTR_FOR_MODIFY, |r| objects::get::<_, RenderContext>(r).map(|c| c.attributes).unwrap_or_default());
    bind!(r, ctx::CLEAR, clear);
    bind!(r, ctx::CLEAR_TEXTURE, |_, t, c| texture::clear(t, c));
    bind!(r, ctx::BIND_RENDER_TARGETS, |r, color, depth, _layer| bind_targets(r, color, depth));
    bind!(r, ctx::BIND_RENDER_TARGETS_1, |r, color| bind_targets(r, color, NativeRef::default()));
    // TODO: a layer's own targets come with ISceneLayer; until then restoring unbinds.
    bind!(r, ctx::RESTORE_RENDER_TARGETS, |r, _layer| bind_targets(r, NativeRef::default(), NativeRef::default()));
    bind!(r, ctx::SET_VIEWPORT, |r, rect| set_viewport(r, rect.x, rect.y, rect.w, rect.h));
    bind!(r, ctx::SET_VIEWPORT_1, |r, v| with(r, |b| b.viewport = v));
    bind!(r, ctx::SET_VIEWPORT_2, set_viewport);
    bind!(r, ctx::GET_VIEWPORT, |r| with(r, |b| b.viewport));
    bind!(r, ctx::SET_SCISSOR_RECT, |r, rect| with(r, |b| b.scissor = Some(rect)));
    bind!(r, ctx::SUBMIT, |_| {});
    bind!(r, ctx::FORCE_FLUSH_GPU, |_| {
        if let Some(gpu) = device::gpu() {
            gpu.wait_idle();
        }
    });
    bind!(r, ctx::BEGIN_PIX_EVENT, |_, _| {});
    bind!(r, ctx::END_PIX_EVENT, |_| {});
    bind!(r, ctx::PIX_SET_MARKER, |_, _| {});
    bind!(r, tools::BIND_COLOR_AND_DEPTH_TARGET, |r, color, depth, _srgb| bind_targets(r, color, depth));
    bind!(r, tools::BIND_DEPTH_TARGET, |r, depth, _slice| bind_targets(r, NativeRef::default(), depth));
    bind!(r, tools::SUPPORTS_DRAW_INDIRECT_COUNT, || false);
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
