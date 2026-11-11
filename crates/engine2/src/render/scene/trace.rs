//! `UNDERBOX_LAYERS=log`: one line per executed layer (view, layer name, kind, target formats and
//! sizes, draws), for finding where a booted client's frame goes black.

use super::layer::{Kind, SceneLayer};
use crate::render::draw::{Draw, Target};
use crate::render::texture;
use std::sync::LazyLock;

static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var("UNDERBOX_LAYERS").is_ok_and(|v| v == "log"));

pub fn enabled() -> bool {
    *ENABLED
}

pub(crate) fn describe(t: texture::Ref) -> String {
    match texture::resolve(t) {
        Some(t) => match &t.gpu {
            Some(g) => format!("{}:{:?}:{}x{}", t.name, g.desc.format, g.desc.width, g.desc.height),
            None => format!("{}:cpu", t.name),
        },
        None if t.is_null() => "-".into(),
        None => "dead".into(),
    }
}

/// One line per `DrawModelDrawCall` (the first 8 per pair of targets): targets, viewport, depth
/// mode, instance ids and the transform each reads.
pub fn draw_call(t: &Target, reverse_z: bool, ids: &[u32], transforms: Option<&[u8]>) {
    static SEEN: LazyLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> = LazyLock::new(Default::default);
    let targets = format!("{} / {}", describe(t.color), describe(t.depth));
    let n = {
        let mut seen = SEEN.lock().unwrap();
        let n = seen.entry(targets.clone()).or_default();
        *n += 1;
        *n
    };
    if !enabled() || n > 8 {
        return;
    }
    let f = |b: &[u8], i: usize| b.get(i * 4..i * 4 + 4).map_or(f32::NAN, |x| f32::from_le_bytes(x.try_into().unwrap()));
    let at: Vec<String> = ids.iter().map(|&id| transforms.map_or("-".into(), |b| { let o = id as usize * 16; format!("{id}@({:.0},{:.0},{:.0})", f(b, o + 3), f(b, o + 7), f(b, o + 11)) })).collect();
    let v = t.viewport;
    let msg = format!(
        "draw call: -> {targets} viewport {}x{}+{}+{} z {}..{} reverse_z {reverse_z} instances [{}] transforms {}",
        v.m_n_width, v.m_n_height, v.m_n_top_left_x, v.m_n_top_left_y, v.min_z, v.max_z, at.join(" "), transforms.map_or(0, <[u8]>::len)
    );
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}

/// The view's eye and forward axis, before its layers.
pub fn view(view: usize, view_proj: glam::Mat4) {
    if !enabled() {
        return;
    }
    let v = crate::render::shading::view::View { view_proj, viewport: [0.0; 4], target: [1.0; 2], tone_map: 1.0 };
    let (eye, forward) = (v.camera_position(), v.camera_axes().0);
    abi::rt::log::log(abi::rt::log::Level::Info, &format!("view: {view:#x} eye ({:.0},{:.0},{:.0}) forward ({:.2},{:.2},{:.2})", eye.x, eye.y, eye.z, forward.x, forward.y, forward.z));
}

pub fn layer(view: usize, layer: &SceneLayer, t: &Target, draws: &[Draw]) {
    if !enabled() {
        return;
    }
    let kind = match layer.kind {
        Kind::Render(mode) => format!("render({:#x})", mode.0),
        Kind::Clear(f) => format!("clear({f:#x})"),
        Kind::Label => "label".into(),
        Kind::Procedural(_) => "procedural".into(),
        Kind::Blit { src, .. } => format!("blit({})", describe(src)),
        Kind::View(child) => format!("view({:#x})", child.addr()),
        Kind::Exposure { src, .. } => format!("exposure({})", describe(src)),
    };
    let msg = format!("layer: view {view:#x} '{}' {kind} -> {} / {} draws {} with material {}", layer.name, describe(t.color), describe(t.depth), draws.len(), draws.iter().filter(|d| d.shaded.is_some()).count());
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}
