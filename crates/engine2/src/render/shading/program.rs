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
    /// A comparison sampler's `ComparisonFunc( ... )`.
    pub compare: Option<wgpu::CompareFunction>,
    /// A texture's `SrgbRead( true )`: sampled through its sRGB view.
    pub srgb: bool,
}

fn compare_func(s: &str) -> Option<wgpu::CompareFunction> {
    use wgpu::CompareFunction as C;
    Some(match s.trim() {
        "NEVER" => C::Never,
        "LESS" => C::Less,
        "EQUAL" => C::Equal,
        "LESS_EQUAL" => C::LessEqual,
        "GREATER" => C::Greater,
        "NOT_EQUAL" => C::NotEqual,
        "GREATER_EQUAL" => C::GreaterEqual,
        "ALWAYS" => C::Always,
        _ => return None,
    })
}

/// A material program (`render::Program`) or a compute program (`render::ComputeProgram`).
pub struct Shading<P = render::Program> {
    pub label: String,
    pub program: P,
    /// Every resource either stage uses, once per (group, binding).
    pub bindings: Vec<Binding>,
    pub vertex_inputs: Vec<VertexInput>,
    pub blocks: Blocks,
    /// Structured-buffer element layouts.
    pub storage: Blocks,
    /// By HLSL variable name.
    pub attributes: HashMap<String, Attr>,
    pub render_state: super::render_state::Overrides,
}

fn stage(compiler: &Compiler, shader: &VfxShader, stage: Stage, v: &Variant) -> Result<CompiledStage, String> {
    let program = shader.program_for(stage).ok_or_else(|| format!("no {stage:?} program"))?;
    compiler.compile(shader, program, v).map_err(|e| format!("{stage:?} {:?}: {}", e.phase, e.message.lines().next().unwrap_or("")))
}

/// Bindings the per-draw binder can't fill: bindless arrays, storage images, external textures.
/// Elements bound for a runtime-sized (bindless) array; every one is a default texture or sampler.
pub const RUNTIME_ARRAY_LEN: u32 = 1024;

/// The element count bound for `b`: 1 for a plain binding.
pub fn array_len(b: &Binding) -> u32 {
    match b.count {
        Some(0) => RUNTIME_ARRAY_LEN,
        Some(n) => n,
        None => 1,
    }
}

fn supported(gpu: &render::Gpu, b: &Binding) -> bool {
    match b.ty {
        wgpu::BindingType::Texture { .. } | wgpu::BindingType::Sampler(_) => {
            b.count.is_none() || gpu.device.features().contains(wgpu::Features::TEXTURE_BINDING_ARRAY)
        }
        wgpu::BindingType::Buffer { .. } | wgpu::BindingType::StorageTexture { .. } => b.count.is_none(),
        _ => false,
    }
}

/// What the binder needs of either program kind.
pub trait Layouts {
    fn bind_group_layouts(&self) -> &[wgpu::BindGroupLayout];
}

impl Layouts for render::Program {
    fn bind_group_layouts(&self) -> &[wgpu::BindGroupLayout] {
        &self.bind_group_layouts
    }
}

impl Layouts for render::ComputeProgram {
    fn bind_group_layouts(&self) -> &[wgpu::BindGroupLayout] {
        &self.bind_group_layouts
    }
}

fn merged(gpu: &render::Gpu, stages: &[&CompiledStage]) -> Result<Vec<Binding>, String> {
    let mut bindings: Vec<Binding> = Vec::new();
    for b in stages.iter().flat_map(|s| &s.reflection.bindings) {
        if !supported(gpu, b) {
            return Err(format!("unsupported binding {} ({:?})", b.name, b.ty));
        }
        if !bindings.iter().any(|x| (x.group, x.binding) == (b.group, b.binding)) {
            bindings.push(b.clone());
        }
    }
    Ok(bindings)
}

fn layouts(gpu: &render::Gpu, label: &str, stages: &[&CompiledStage]) -> Vec<wgpu::BindGroupLayout> {
    let reflections: Vec<_> = stages.iter().map(|s| &s.reflection).collect();
    shaders::bind_group_layouts(&reflections, RUNTIME_ARRAY_LEN)
        .iter()
        .map(|entries| gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries }))
        .collect()
}

/// A vfx shader's `CS` program for one combo.
pub fn build_compute(gpu: &render::Gpu, compiler: &Compiler, shader: &VfxShader, label: &str, v: &Variant) -> Result<Shading<render::ComputeProgram>, String> {
    let cs = stage(compiler, shader, Stage::Cs, v)?;
    let bindings = merged(gpu, &[&cs])?;
    let program = gpu.checked(|| {
        let stage = render::Stage::new(gpu, label, &cs.wgsl, &cs.reflection.entry);
        render::ComputeProgram::new(gpu, label, &stage, layouts(gpu, label, &[&cs]), cs.reflection.workgroup_size)
    })?;
    let (blocks, storage) = (layout::uniform_blocks(&cs.wgsl), layout::storage_elements(&cs.wgsl));
    Ok(Shading { label: label.into(), program, bindings, vertex_inputs: Vec::new(), blocks, storage, attributes: attributes(shader), render_state: Default::default() })
}

fn parse_default(args: &str) -> Option<Vec4> {
    // HLSL float literals may carry an `f` suffix (Fog.hlsl's `Default3( 1.0f, 1.0f, 1.0f )`).
    let v: Vec<f32> = args.split(',').map(|s| s.trim().trim_end_matches(['f', 'F']).parse().ok()).collect::<Option<_>>()?;
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    (!v.is_empty()).then(|| Vec4::new(at(0), at(1), at(2), at(3)))
}

fn attributes(shader: &VfxShader) -> HashMap<String, Attr> {
    let mut out = HashMap::new();
    for a in shader.programs.iter().flat_map(|p| &p.annotations) {
        let attribute = a.get("Attribute").map(|s| s.trim().trim_matches('"').to_owned());
        let default = ["Default", "Default1", "Default2", "Default3", "Default4"].iter().find_map(|k| a.get(k).and_then(parse_default));
        let compare = a.get("ComparisonFunc").and_then(compare_func);
        let srgb = a.get("SrgbRead").is_some_and(|v| matches!(v.trim(), "true" | "1"));
        out.entry(a.name.clone()).or_insert(Attr { attribute, default, compare, srgb });
    }
    out
}

pub fn build(gpu: &render::Gpu, compiler: &Compiler, shader: &VfxShader, label: &str, v: &Variant) -> Result<Shading, String> {
    let vs = stage(compiler, shader, Stage::Vs, v)?;
    let mut ps = stage(compiler, shader, Stage::Ps, v)?;
    super::stages::separate(&vs, &mut ps)?;
    let bindings = merged(gpu, &[&vs, &ps])?;
    let program = gpu.checked(|| {
        let vertex = render::Stage::new(gpu, label, &vs.wgsl, &vs.reflection.entry);
        let fragment = render::Stage::new(gpu, label, &ps.wgsl, &ps.reflection.entry);
        render::Program::from_stages(gpu, label, vertex, Some(fragment), layouts(gpu, label, &[&vs, &ps]))
    })?;
    let mut blocks = layout::uniform_blocks(&vs.wgsl);
    blocks.extend(layout::uniform_blocks(&ps.wgsl));
    let mut storage = layout::storage_elements(&vs.wgsl);
    storage.extend(layout::storage_elements(&ps.wgsl));
    Ok(Shading { label: label.into(), program, bindings, vertex_inputs: vs.reflection.vertex_inputs, blocks, storage, attributes: attributes(shader), render_state: super::render_state::of(shader, v) })
}
