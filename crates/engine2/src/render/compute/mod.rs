//! `RenderTools.Compute`: dispatches of compute shaders. There is no GPU compute path yet (the
//! translator builds Vs/Ps programs only); shaders whose outputs other draws depend on run on the
//! CPU instead, the rest are skipped with a warning once per shader.

mod occlusion;

pub(crate) use occlusion::words;

use crate::render::{attributes, buffers, material_mode};
use abi::slots::render_tools as tools;
use abi::{Registry, bind};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

fn dispatch(attrs: attributes::Ref, mode: material_mode::Ref) {
    let Some(material) = material_mode::material(mode) else { return };
    let shader = material.shader();
    if shader.ends_with(occlusion::SHADER) {
        occlusion::cull(&Inputs(attrs));
        return;
    }
    // TODO: translate compute programs and dispatch them on the GPU.
    static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    if WARNED.lock().unwrap().insert(shader.to_owned()) {
        abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{shader}: compute dispatch skipped (no GPU compute yet)"));
    }
}

/// A dispatch's attributes, read the way the shader reads them.
pub(crate) struct Inputs(attributes::Ref);

impl Inputs {
    fn buffer(&self, name: &str) -> Option<std::sync::Arc<render::Buffer>> {
        attributes::buffer(self.0, name).and_then(|h| buffers::get(buffers::Handle::new(h)))
    }

    fn vector(&self, name: &str) -> Option<glam::Vec4> {
        attributes::shader_value(self.0, name)
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::COMPUTE, |_, attrs, mode, _, _, _| dispatch(attrs, mode));
}
