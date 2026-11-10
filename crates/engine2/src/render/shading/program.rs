//! One material program: a vfx shader's vertex and pixel stages for one (static, dynamic) combo,
//! translated by crates/shaders and built into a `render::Program`, with what binding needs:
//! the merged resource list, the vertex inputs, uniform member layouts and attribute names.

use super::layout::{self, Blocks};
use glam::Vec4;
use render::wgpu;
use shaders::reflect::{Binding, VertexInput};
use shaders::{CompiledStage, Compiler, Stage, Variant, VfxShader};
use std::collections::HashMap;

/// What a vfx global answers to: its `Attribute( "Name" )` and `Default*( ... )`.
#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub attribute: Option<String>,
    pub default: Option<Vec4>,
}

pub struct Shading {
    pub label: String,
    pub program: render::Program,
    /// Every resource either stage uses, once per (group, binding).
    pub bindings: Vec<Binding>,
    pub vertex_inputs: Vec<VertexInput>,
    pub blocks: Blocks,
    /// By HLSL variable name.
    pub attributes: HashMap<String, Attr>,
}

fn stage(compiler: &Compiler, shader: &VfxShader, stage: Stage, v: &Variant) -> Result<CompiledStage, String> {
    let program = shader.program_for(stage).ok_or_else(|| format!("no {stage:?} program"))?;
    compiler.compile(shader, program, v).map_err(|e| format!("{stage:?} {:?}: {}", e.phase, e.message.lines().next().unwrap_or("")))
}

/// Bindings the per-draw binder can't fill: bindless arrays, storage images, external textures.
fn supported(b: &Binding) -> bool {
    b.count.is_none() && matches!(b.ty, wgpu::BindingType::Buffer { .. } | wgpu::BindingType::Texture { .. } | wgpu::BindingType::Sampler(_))
}

fn parse_default(args: &str) -> Option<Vec4> {
    let v: Vec<f32> = args.split(',').map(|s| s.trim().parse().ok()).collect::<Option<_>>()?;
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    (!v.is_empty()).then(|| Vec4::new(at(0), at(1), at(2), at(3)))
}

fn attributes(shader: &VfxShader) -> HashMap<String, Attr> {
    let mut out = HashMap::new();
    for a in shader.programs.iter().flat_map(|p| &p.annotations) {
        let attribute = a.get("Attribute").map(|s| s.trim().trim_matches('"').to_owned());
        let default = ["Default", "Default1", "Default2", "Default3", "Default4"].iter().find_map(|k| a.get(k).and_then(parse_default));
        out.entry(a.name.clone()).or_insert(Attr { attribute, default });
    }
    out
}

pub fn build(gpu: &render::Gpu, compiler: &Compiler, shader: &VfxShader, label: &str, v: &Variant) -> Result<Shading, String> {
    let vs = stage(compiler, shader, Stage::Vs, v)?;
    let mut ps = stage(compiler, shader, Stage::Ps, v)?;
    super::stages::separate(&vs, &mut ps)?;
    let mut bindings: Vec<Binding> = Vec::new();
    for b in vs.reflection.bindings.iter().chain(&ps.reflection.bindings) {
        if !supported(b) {
            return Err(format!("unsupported binding {} ({:?})", b.name, b.ty));
        }
        if !bindings.iter().any(|x| (x.group, x.binding) == (b.group, b.binding)) {
            bindings.push(b.clone());
        }
    }
    let program = gpu.checked(|| {
        let layouts = shaders::bind_group_layouts(&[&vs.reflection, &ps.reflection], 1)
            .iter()
            .map(|entries| gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries }))
            .collect();
        let vertex = render::Stage::new(gpu, label, &vs.wgsl, &vs.reflection.entry);
        let fragment = render::Stage::new(gpu, label, &ps.wgsl, &ps.reflection.entry);
        render::Program::from_stages(gpu, label, vertex, Some(fragment), layouts)
    })?;
    let mut blocks = layout::uniform_blocks(&vs.wgsl);
    blocks.extend(layout::uniform_blocks(&ps.wgsl));
    Ok(Shading { label: label.into(), program, bindings, vertex_inputs: vs.reflection.vertex_inputs, blocks, attributes: attributes(shader) })
}
