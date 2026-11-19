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
    depth_slice: u32,
    viewport: RenderViewport,
    scissor: Option<NativeRect>,
    srgb_write: bool,
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
    with(r, |b| Target { color: b.color.unwrap_or_default(), depth: b.depth.unwrap_or_default(), depth_slice: b.depth_slice, viewport: b.viewport, scissor: b.scissor, srgb_write: b.srgb_write })
}

/// Bind `t`'s targets, viewport, scissor and sRGB write at once.
pub fn bind_target(r: Ref, t: &Target) {
    bind_targets_srgb(r, t.color, t.depth, t.srgb_write);
    with(r, |b| {
        b.depth_slice = t.depth_slice;
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
    bind_targets_srgb(r, color, depth, false);
}

/// [`bind_targets`] with native's sRGB write: an 8-bit colour target stores sRGB-encoded values
/// (swap chains always, `BindColorAndDepthTarget` when asked).
pub fn bind_targets_srgb(r: Ref, color: texture::Ref, depth: texture::Ref, srgb_write: bool) {
    let size = some(color).or(some(depth)).and_then(texture::resolve).and_then(|t| t.gpu.as_ref().map(|g| (g.desc.width, g.desc.height)));
    with(r, |b| {
        (b.color, b.depth, b.depth_slice, b.scissor, b.srgb_write) = (some(color), some(depth), 0, None, srgb_write);
        if let Some((w, h)) = size {
            b.viewport = RenderViewport { m_n_width: w as i32, m_n_height: h as i32, max_z: 1.0, ..Default::default() };
        }
    });
}

pub fn srgb_write(r: Ref) -> bool {
    with(r, |b| b.srgb_write)
}

/// Clears the bound targets within the viewport and scissor. Depth clears to 0, the far plane:
/// every view is reverse-Z, as native's.
pub(crate) fn clear(r: Ref, color: Vector4, clear_color: bool, clear_depth: bool, _clear_stencil: bool) {
    let Some(gpu) = device::gpu() else { return };
    let (target, depth, slice, viewport, scissor) = with(r, |b| (b.color, b.depth, b.depth_slice, b.viewport, b.scissor));
    crate::render::submits::log(r, &format!("clear color {clear_color} depth {clear_depth}"));
    let v = color.vec;
    let rgba = render::wgpu::Color { r: v.x.into(), g: v.y.into(), b: v.z.into(), a: v.w.into() };
    let clear_in = |t: Option<texture::Ref>, depth: f32, layer: u32| {
        let Some(t) = t.and_then(texture::resolve) else { return };
        let Some(tex) = &t.gpu else { return };
        let rect = clear_rect(tex, &viewport, scissor);
        if rect[2] == 0 || rect[3] == 0 {
            return;
        }
        let partial = rect != [0, 0, tex.desc.width, tex.desc.height] && rect_clearer(gpu).clear(gpu, tex, layer, rect, rgba, depth);
        if !partial {
            tex.clear_layer(gpu, rgba, depth, layer);
        }
    };
    if clear_color {
        clear_in(target, 1.0, 0);
    }
    if clear_depth {
        clear_in(depth, crate::render::depth::VIEW.clear(), slice);
    }
}

/// The viewport (an empty one is the whole target) within the scissor, clamped to `t`.
fn clear_rect(t: &render::Texture, viewport: &RenderViewport, scissor: Option<NativeRect>) -> [u32; 4] {
    let [x, y, w, h] = crate::render::draw::viewport_rect(t, viewport);
    let (mut x0, mut y0, mut x1, mut y1) = (x, y, (x + w).min(t.desc.width), (y + h).min(t.desc.height));
    if let Some(s) = scissor.filter(|s| s.w > 0 && s.h > 0) {
        let (sx, sy) = (s.x.max(0) as u32, s.y.max(0) as u32);
        (x0, y0, x1, y1) = (x0.max(sx), y0.max(sy), x1.min(sx + s.w as u32), y1.min(sy + s.h as u32));
    }
    [x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)]
}

fn rect_clearer(gpu: &render::Gpu) -> &'static render::RectClearer {
    static CLEARER: std::sync::OnceLock<render::RectClearer> = std::sync::OnceLock::new();
    CLEARER.get_or_init(|| render::RectClearer::new(gpu))
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
    // Managed sets the viewport itself right after (NativeFrameView.RestoreTargets).
    bind!(r, ctx::RESTORE_RENDER_TARGETS, |r, layer| {
        let (color, depth) = crate::render::scene::layer::targets(layer);
        bind_targets_srgb(r, color, depth, crate::render::scene::layer::srgb_output(layer));
    });
    bind!(r, ctx::SET_VIEWPORT, |r, rect| set_viewport(r, rect.x, rect.y, rect.w, rect.h));
    bind!(r, ctx::SET_VIEWPORT_1, |r, v| with(r, |b| b.viewport = v));
    bind!(r, ctx::SET_VIEWPORT_2, set_viewport);
    bind!(r, ctx::GET_VIEWPORT, |r| with(r, |b| b.viewport));
    bind!(r, ctx::SET_SCISSOR_RECT, |r, rect| with(r, |b| b.scissor = Some(rect)));
    bind!(r, ctx::SUBMIT, |c| crate::render::submits::log(c, "Submit"));
    bind!(r, ctx::FORCE_FLUSH_GPU, |_| {
        if let Some(gpu) = device::gpu() {
            gpu.wait_idle();
        }
    });
    bind!(r, ctx::BEGIN_PIX_EVENT, crate::render::submits::push_scope);
    bind!(r, ctx::END_PIX_EVENT, crate::render::submits::pop_scope);
    bind!(r, ctx::PIX_SET_MARKER, |_, _| {});
    bind!(r, tools::BIND_COLOR_AND_DEPTH_TARGET, bind_targets_srgb);
    bind!(r, tools::BIND_DEPTH_TARGET, |r, depth, slice| {
        bind_targets(r, NativeRef::default(), depth);
        with(r, |b| b.depth_slice = slice.max(0) as u32);
    });
    bind!(r, tools::SUPPORTS_DRAW_INDIRECT_COUNT, || false);
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
