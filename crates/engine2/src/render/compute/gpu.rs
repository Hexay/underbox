//! Compute dispatches on the GPU: the shader's `CS` program (shading::compute), bound by
//! reflected name from the dispatch's attributes, then the context's (constant blocks such as
//! `PerViewConstantBuffer_t` and `ClusteredLightingConstants` live there), over the requested threads.

use crate::render::attributes;
use crate::render::lighting::Lighting;
use crate::render::shading::{self, bind, texel_words, view::View};
use std::collections::HashMap;

/// How much work a dispatch does.
pub enum Work<'a> {
    Threads([u32; 3]),
    /// `RenderTools.ComputeIndirect`: workgroup counts read on the GPU at a byte offset.
    Indirect(&'a render::wgpu::Buffer, u64),
}

/// Run `shader` over `work` outside any view; `Err` when it doesn't translate or bind.
pub fn dispatch(gpu: &render::Gpu, shader: &str, attrs: &[attributes::Ref], work: Work) -> Result<(), String> {
    dispatch_in(gpu, shader, attrs, work, &View::default(), &Lighting::default())
}

/// [`dispatch`] inside a view: per-view constants and the lighting blocks come from `view` and
/// `lighting` where the attributes don't carry them (clustered_light_culling_cs bins these lights).
pub fn dispatch_in(gpu: &render::Gpu, shader: &str, attrs: &[attributes::Ref], work: Work, view: &View, lighting: &Lighting) -> Result<(), String> {
    if crate::render::scene::trace::skipped(shader.rsplit('/').next().unwrap_or(shader).trim_end_matches(".shader")) {
        return Ok(());
    }
    let program = shading::compute(gpu, shader, attrs)?;
    let textures = HashMap::new();
    let inputs = bind::Inputs { view, lighting, instances: &[], textures: &textures, values: &[], attributes: attrs, slots: &[], targets: &[] };
    let groups = gpu.checked(|| bind::bind_groups(gpu, &program, &inputs)).and_then(|g| g)?;
    let words = texel_words::Synced::load(gpu, &program, &inputs);
    let ran = gpu.checked(|| match work {
        Work::Threads(threads) => program.program.dispatch(gpu, &groups, threads),
        Work::Indirect(args, offset) => program.program.dispatch_indirect(gpu, &groups, args, offset),
    });
    words.store(gpu);
    ran
}
