//! `RenderTools.Compute`: compute shaders run on the GPU (gpu.rs) when their `CS` program
//! translates; those that don't fall back to a CPU port when other draws depend on their output
//! (occlusion.rs), else they are skipped with a warning once per shader.

pub(crate) mod gpu;
mod occlusion;
mod pvs_cull;

pub(crate) use occlusion::words;

use crate::render::draw::immediate;
use crate::render::lighting::Lighting;
use crate::render::shading::view::View;
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

/// What `Compute` or `ComputeIndirect` asked for.
enum Size {
    Threads([i32; 3]),
    Indirect(buffers::Handle, u32),
}

fn dispatch(c: context::Ref, attrs: attributes::Ref, mode: material_mode::Ref, size: Size) {
    let (Some(material), Some(gpu)) = (material_mode::material(mode), device::gpu()) else { return };
    let shader = material.shader();
    crate::render::submits::log_to(c, &format!("compute {shader}"), "-");
    let occlusion = shader.ends_with(occlusion::SHADER);
    let given = attrs;
    let attrs = if occlusion { pvs_cull::filter(attrs) } else { attrs };
    let mut layers = vec![attrs];
    layers.extend(context::get(c).map(|c| c.attributes));
    if shader.contains(crate::render::fog_managed::SHADER) {
        if let Err(e) = crate::render::fog_managed::dispatch(&layers) {
            warn_once(shader, &format!("fog stand-in skipped ({e})"));
        }
        return;
    }
    let args = match size {
        Size::Indirect(h, _) => buffers::get(h),
        Size::Threads(_) => None,
    };
    let work = match (size, &args) {
        (Size::Threads(threads), _) => Ok(gpu::Work::Threads(threads.map(|n| n.max(1) as u32))),
        (Size::Indirect(_, offset), Some(args)) => Ok(gpu::Work::Indirect(&args.buffer, u64::from(offset))),
        (Size::Indirect(..), None) => Err("no indirect args buffer".to_owned()),
    };
    let (view, lighting) = view_of(c);
    match work.and_then(|work| gpu::dispatch_in(gpu, shader, &layers, work, &view, &lighting)) {
        Ok(()) => {}
        Err(e) if occlusion => {
            warn_once(shader, &format!("on the CPU ({e})"));
            occlusion::cull(&Inputs(attrs));
        }
        Err(e) => warn_once(shader, &format!("compute dispatch skipped ({e})")),
    }
    if occlusion {
        pvs_cull::release(attrs, given);
    }
}

/// The view a dispatch on `c` runs in: the scene view's camera and lighting, and the bound target's viewport.
fn view_of(c: context::Ref) -> (View, std::sync::Arc<Lighting>) {
    let vp = context::target(c).viewport;
    let size = [vp.m_n_width.max(1) as f32, vp.m_n_height.max(1) as f32];
    let view = View {
        view_proj: immediate::view_proj(c),
        viewport: [vp.m_n_top_left_x as f32, vp.m_n_top_left_y as f32, size[0], size[1]],
        target: size,
        depth_range: [vp.min_z.max(0.0), if vp.max_z > vp.min_z { vp.max_z } else { 1.0 }],
        ..View::default()
    };
    (view, immediate::lighting(c))
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
#[path = "cmaa2_tests.rs"]
mod cmaa2_tests;
#[cfg(test)]
#[path = "gpu_tests.rs"]
mod gpu_tests;
#[cfg(test)]
#[path = "image_atomics_tests.rs"]
mod image_atomics_tests;
#[cfg(test)]
#[path = "indirect_tests.rs"]
mod indirect_tests;
#[cfg(test)]
#[path = "packed_tests.rs"]
mod packed_tests;
#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;

pub fn register(r: &mut Registry) {
    bind!(r, tools::COMPUTE, |c, attrs, mode, x, y, z| dispatch(c, attrs, mode, Size::Threads([x, y, z])));
    bind!(r, tools::COMPUTE_INDIRECT, |c, attrs, mode, args, offset| dispatch(c, attrs, mode, Size::Indirect(args, offset)));
}
