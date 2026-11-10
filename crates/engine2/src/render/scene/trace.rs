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

fn describe(t: texture::Ref) -> String {
    match texture::resolve(t) {
        Some(t) => match &t.gpu {
            Some(g) => format!("{}:{:?}:{}x{}", t.name, g.desc.format, g.desc.width, g.desc.height),
            None => format!("{}:cpu", t.name),
        },
        None if t.is_null() => "-".into(),
        None => "dead".into(),
    }
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
    };
    let msg = format!("layer: view {view:#x} '{}' {kind} -> {} / {} draws {} with material {}", layer.name, describe(t.color), describe(t.depth), draws.len(), draws.iter().filter(|d| d.shaded.is_some()).count());
    abi::rt::log::log(abi::rt::log::Level::Info, &msg);
}
