//! `RenderTools.Compute`: compute shaders run on the GPU (gpu.rs) when their `CS` program
//! translates; those that don't fall back to a CPU port when other draws depend on their output
//! (occlusion.rs), else they are skipped with a warning once per shader.

pub(crate) mod gpu;
mod occlusion;

pub(crate) use occlusion::words;

use crate::render::{attributes, buffers, context, device, material_mode};
use abi::slots::render_tools as tools;
use abi::{Registry, bind};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

pub(crate) fn warn_once(shader: &str, what: &str) {
    static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    if WARNED.lock().unwrap().insert(shader.to_owned()) {
        abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{shader}: {what}"));
    }
}

fn dispatch(c: context::Ref, attrs: attributes::Ref, mode: material_mode::Ref, threads: [i32; 3]) {
    let (Some(material), Some(gpu)) = (material_mode::material(mode), device::gpu()) else { return };
    let shader = material.shader();
    crate::render::submits::log_to(c, &format!("compute {shader}"), "-");
    let mut layers = vec![attrs];
    layers.extend(context::get(c).map(|c| c.attributes));
    let threads = threads.map(|n| n.max(1) as u32);
    let Err(e) = gpu::dispatch(gpu, shader, &layers, threads) else { return };
    if shader.ends_with(occlusion::SHADER) {
        warn_once(shader, &format!("on the CPU ({e})"));
        occlusion::cull(&Inputs(attrs));
        return;
    }
    warn_once(shader, &format!("compute dispatch skipped ({e})"));
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

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod gpu_tests;

pub fn register(r: &mut Registry) {
    bind!(r, tools::COMPUTE, |c, attrs, mode, x, y, z| dispatch(c, attrs, mode, [x, y, z]));
}
