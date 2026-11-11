//! Compute dispatches on the GPU: the shader's `CS` program (shading::compute), bound by
//! reflected name from the dispatch's attributes, then the context's (constant blocks such as
//! `PerViewConstantBuffer_t` and `ClusteredLightingConstants` live there), over the requested threads.

use crate::render::attributes;
use crate::render::lighting::Lighting;
use crate::render::shading::{self, bind, view::View};
use std::collections::HashMap;

/// Run `shader` over `threads`; `Err` when it doesn't translate or bind.
pub fn dispatch(gpu: &render::Gpu, shader: &str, attrs: &[attributes::Ref], threads: [u32; 3]) -> Result<(), String> {
    let program = shading::compute(gpu, shader, attrs)?;
    let view = View { view_proj: glam::Mat4::IDENTITY, viewport: [0.0; 4], target: [1.0; 2] };
    let (lighting, textures) = (Lighting::default(), HashMap::new());
    let inputs = bind::Inputs { view: &view, lighting: &lighting, instances: &[], textures: &textures, values: &[], attributes: attrs, slots: &[], targets: &[] };
    let groups = gpu.checked(|| bind::bind_groups(gpu, &program, &inputs)).and_then(|g| g)?;
    gpu.checked(|| program.program.dispatch(gpu, &groups, threads))
}
