//! `UNDERBOX_SUBMITS=log`: one line per GPU submission a context makes (draws, clears, copies,
//! blits, dispatches) and per managed `Submit`, in the order they reach the queue. Each names the
//! frame, the context, its GPU scope (managed's debug markers, which this turns on), the view's
//! camera position and the targets, for checking that a pass's producers ran before its reads.

use crate::render::{attributes, context, texture};
use crate::render::scene::trace::describe;
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var("UNDERBOX_SUBMITS").is_ok_and(|v| v == "log"));
static FRAME: AtomicU32 = AtomicU32::new(0);
static SEQ: AtomicU32 = AtomicU32::new(0);
static SCOPES: LazyLock<Mutex<HashMap<usize, Vec<String>>>> = LazyLock::new(Default::default);

pub fn enabled() -> bool {
    *ENABLED
}

/// `BeginRenderingViews`: number the frame's submissions from 0.
pub fn begin_frame() {
    FRAME.fetch_add(1, Ordering::Relaxed);
    SEQ.store(0, Ordering::Relaxed);
}

/// `BeginPixEvent(name)`: managed's GPU scope name (a C string).
pub fn push_scope(c: context::Ref, name: abi::RawPtr<std::ffi::c_void>) {
    if !enabled() || name.is_null() {
        return;
    }
    // SAFETY: managed passes a NUL-terminated name it keeps alive for the scope (RenderContext.BeginGpuScope).
    let name = unsafe { CStr::from_ptr(name.cast::<c_char>().as_ptr()) }.to_string_lossy().into_owned();
    SCOPES.lock().unwrap().entry(c.addr()).or_default().push(name);
}

pub fn pop_scope(c: context::Ref) {
    if enabled() {
        SCOPES.lock().unwrap().get_mut(&c.addr()).and_then(Vec::pop);
    }
}

/// The view's camera position from `PerViewConstantBuffer_t` (`g_vCameraPositionWs`) and the sun's
/// first cascade map index from `DirectionalLightCB`, when the context's attributes have them.
fn eye(c: context::Ref) -> String {
    let Some(attrs) = context::get(c).map(|x| x.attributes) else { return "-".into() };
    let f = |b: &[u8], i: usize| b.get(i..i + 4).map_or(f32::NAN, |x| f32::from_le_bytes(x.try_into().unwrap()));
    let view = attributes::constant_buffer(attrs, crate::render::shading::view::BLOCK);
    let eye = view.map_or_else(|| "-".into(), |b| format!("({:.0},{:.0},{:.0})", f(&b, 304), f(&b, 308), f(&b, 312)));
    let sun = attributes::constant_buffer(attrs, "DirectionalLightCB").and_then(|b| b.get(288..292).map(|x| u32::from_le_bytes(x.try_into().unwrap())));
    let map = |i: u32| crate::render::bindless::at(i).map(|(t, _)| t).map_or_else(|| "none".into(), |t| format!("{}@{:#x}", describe(t), t.addr()));
    sun.map_or(eye.clone(), |i| format!("{eye} sun map {i} = {}", map(i)))
}

/// Log `what` on `c` into its bound targets.
pub fn log(c: context::Ref, what: &str) {
    if enabled() {
        let t = context::target(c);
        log_to(c, what, &format!("{}@{:#x} / {}@{:#x}", describe(t.color), t.color.addr(), describe(t.depth), t.depth.addr()));
    }
}

/// Log `what` on `c` into `target` (copies name their destination).
pub fn log_to(c: context::Ref, what: &str, target: &str) {
    if !enabled() {
        return;
    }
    let scope = SCOPES.lock().unwrap().get(&c.addr()).map(|s| s.join("/")).unwrap_or_default();
    let (frame, seq) = (FRAME.load(Ordering::Relaxed), SEQ.fetch_add(1, Ordering::Relaxed));
    let msg = format!("submit: f{frame} #{seq} ctx {:#x} [{scope}] {what} -> {target} view {}", c.addr(), eye(c));
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}

/// [`log_to`] naming a texture.
pub fn log_texture(c: context::Ref, what: &str, t: texture::Ref) {
    if enabled() {
        log_to(c, what, &describe(t));
    }
}
