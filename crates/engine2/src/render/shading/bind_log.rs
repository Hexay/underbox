//! `UNDERBOX_BIND_LOG=<substring>`: for programs whose label contains it, where each binding's
//! value comes from (an attribute, or the binder's zero/default stand-in), and which uniform
//! members nothing supplies. Once per (program, line).

use super::bind::{Inputs, names, uniform_value};
use super::program::Shading;
use super::view;
use crate::render::attributes;
use render::wgpu::{self, BindingType as B};
use shaders::reflect::Binding;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

static FILTER: LazyLock<Option<String>> = LazyLock::new(|| std::env::var("UNDERBOX_BIND_LOG").ok().filter(|s| !s.is_empty()));

pub fn enabled(label: &str) -> bool {
    FILTER.as_deref().is_some_and(|f| label.contains(f))
}

fn note(label: &str, line: String) {
    static SEEN: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Default::default);
    if SEEN.lock().unwrap().insert((label.to_owned(), line.clone())) {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("bind {label}: {line}"));
    }
}

pub fn describe<P>(s: &Shading<P>, inputs: &Inputs, bindings: &[&Binding]) {
    for b in bindings {
        let attr = |f: &dyn Fn(attributes::Ref, &str) -> bool| inputs.attributes.iter().any(|a| names(s, &b.name).any(|n| f(*a, n)));
        let line = match b.ty {
            B::Buffer { ty: wgpu::BufferBindingType::Uniform, .. } => {
                if attr(&|a, n| attributes::constant_buffer(a, n).is_some()) {
                    format!("{} block from attributes", b.name)
                } else if b.name == view::BLOCK {
                    format!("{} per-view", b.name)
                } else {
                    let block = s.blocks.get(&(b.group, b.binding));
                    let missing: Vec<&str> = block.into_iter().flat_map(|k| &k.fields).map(|f| f.name.as_str()).filter(|n| uniform_value(s, inputs, n).is_none()).collect();
                    format!("{} block, unset: {missing:?}", b.name)
                }
            }
            B::Buffer { .. } => format!("{} buffer {}", b.name, if attr(&|a, n| attributes::buffer(a, n).is_some()) { "from attributes" } else { "zeros" }),
            B::Texture { .. } => format!("{} texture {}", b.name, if attr(&|a, n| attributes::texture(a, n).is_some()) { "from attributes" } else { "material or default" }),
            _ => continue,
        };
        note(&s.label, line);
    }
}
