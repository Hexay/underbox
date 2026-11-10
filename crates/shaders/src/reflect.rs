//! Reflection from the naga module: the resources an entry point uses (as wgpu bind-group layout
//! entries), its vertex inputs with their HLSL semantics, immediates and workgroup size. This is
//! what `render::Program::new` needs to build layouts; see docs/renderer.md, "Shaders".

use crate::shader::Stage;
use crate::spirv::InputSemantic;
use naga::{AddressSpace, ArraySize, ImageClass, ImageDimension, ScalarKind, TypeInner};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use wgpu_types as wgt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub ty: wgt::BindingType,
    /// Binding-array length; `Some(0)` for a runtime-sized (bindless) array.
    pub count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VertexInput {
    pub location: u32,
    /// HLSL semantic (`POSITION`, `TEXCOORD0`), from DXC's `-fspv-reflect`.
    pub semantic: String,
    pub name: String,
    pub format: Option<wgt::VertexFormat>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reflection {
    pub stage: Stage,
    pub entry: String,
    pub bindings: Vec<Binding>,
    pub vertex_inputs: Vec<VertexInput>,
    /// Bytes of immediate (push-constant) data the entry point reads.
    pub immediate_size: u32,
    pub workgroup_size: [u32; 3],
}

pub fn shader_stages(stage: Stage) -> wgt::ShaderStages {
    match stage {
        Stage::Vs => wgt::ShaderStages::VERTEX,
        Stage::Ps => wgt::ShaderStages::FRAGMENT,
        Stage::Cs => wgt::ShaderStages::COMPUTE,
        _ => wgt::ShaderStages::NONE,
    }
}

pub fn reflect(module: &naga::Module, info: &naga::valid::ModuleInfo, stage: Stage, semantics: &[InputSemantic]) -> Result<Reflection, String> {
    let (index, ep) = module.entry_points.iter().enumerate().next().ok_or("module has no entry point")?;
    let uses = info.get_entry_point(index);
    let mut bindings = Vec::new();
    let mut immediate_size = 0;
    for (handle, gv) in module.global_variables.iter() {
        if uses[handle].is_empty() {
            continue;
        }
        let size = module.types[gv.ty].inner.size(module.to_ctx());
        if gv.space == AddressSpace::Immediate {
            immediate_size = size;
            continue;
        }
        let Some(rb) = &gv.binding else { continue };
        let (ty, count) = binding_type(module, gv.ty, gv.space)?;
        bindings.push(Binding { group: rb.group, binding: rb.binding, name: gv.name.clone().unwrap_or_default(), ty, count });
    }
    bindings.sort_by_key(|b| (b.group, b.binding));
    let mut vertex_inputs = Vec::new();
    if stage == Stage::Vs {
        for arg in &ep.function.arguments {
            collect_inputs(module, arg.ty, arg.binding.as_ref(), semantics, &mut vertex_inputs);
        }
        vertex_inputs.sort_by_key(|v| v.location);
    }
    Ok(Reflection { stage, entry: ep.name.clone(), bindings, vertex_inputs, immediate_size, workgroup_size: ep.workgroup_size })
}

fn binding_type(module: &naga::Module, ty: naga::Handle<naga::Type>, space: AddressSpace) -> Result<(wgt::BindingType, Option<u32>), String> {
    let inner = &module.types[ty].inner;
    if let TypeInner::BindingArray { base, size } = inner {
        let count = match size {
            ArraySize::Constant(n) => n.get(),
            _ => 0,
        };
        return Ok((binding_type(module, *base, space)?.0, Some(count)));
    }
    let buffer = |ty| wgt::BindingType::Buffer {
        ty,
        has_dynamic_offset: false,
        min_binding_size: wgt::BufferSize::new(inner.size(module.to_ctx()).into()),
    };
    Ok((
        match (space, inner) {
            (AddressSpace::Uniform, _) => buffer(wgt::BufferBindingType::Uniform),
            (AddressSpace::Storage { access }, _) => {
                buffer(wgt::BufferBindingType::Storage { read_only: !access.contains(naga::StorageAccess::STORE) })
            }
            (_, TypeInner::Sampler { comparison }) => wgt::BindingType::Sampler(if *comparison {
                wgt::SamplerBindingType::Comparison
            } else {
                wgt::SamplerBindingType::Filtering
            }),
            (_, TypeInner::Image { dim, arrayed, class }) => image(*dim, *arrayed, class)?,
            (_, TypeInner::AccelerationStructure { vertex_return }) => {
                wgt::BindingType::AccelerationStructure { vertex_return: *vertex_return }
            }
            (space, other) => return Err(format!("unsupported resource {space:?} {other:?}")),
        },
        None,
    ))
}

fn image(dim: ImageDimension, arrayed: bool, class: &ImageClass) -> Result<wgt::BindingType, String> {
    let view_dimension = match (dim, arrayed) {
        (ImageDimension::D1, _) => wgt::TextureViewDimension::D1,
        (ImageDimension::D2, false) => wgt::TextureViewDimension::D2,
        (ImageDimension::D2, true) => wgt::TextureViewDimension::D2Array,
        (ImageDimension::D3, _) => wgt::TextureViewDimension::D3,
        (ImageDimension::Cube, false) => wgt::TextureViewDimension::Cube,
        (ImageDimension::Cube, true) => wgt::TextureViewDimension::CubeArray,
    };
    Ok(match class {
        ImageClass::Sampled { kind, multi } => wgt::BindingType::Texture {
            sample_type: match kind {
                ScalarKind::Sint => wgt::TextureSampleType::Sint,
                ScalarKind::Uint => wgt::TextureSampleType::Uint,
                _ => wgt::TextureSampleType::Float { filterable: !multi },
            },
            view_dimension,
            multisampled: *multi,
        },
        ImageClass::Depth { multi } => {
            wgt::BindingType::Texture { sample_type: wgt::TextureSampleType::Depth, view_dimension, multisampled: *multi }
        }
        ImageClass::Storage { format, access } => wgt::BindingType::StorageTexture {
            access: if access.contains(naga::StorageAccess::LOAD | naga::StorageAccess::STORE) {
                wgt::StorageTextureAccess::ReadWrite
            } else if access.contains(naga::StorageAccess::STORE) {
                wgt::StorageTextureAccess::WriteOnly
            } else {
                wgt::StorageTextureAccess::ReadOnly
            },
            format: crate::formats::storage_format(*format),
            view_dimension,
        },
        ImageClass::External => wgt::BindingType::ExternalTexture,
    })
}

fn collect_inputs(module: &naga::Module, ty: naga::Handle<naga::Type>, binding: Option<&naga::Binding>, semantics: &[InputSemantic], out: &mut Vec<VertexInput>) {
    match (binding, &module.types[ty].inner) {
        (Some(naga::Binding::Location { location, .. }), inner) => {
            let s = semantics.iter().find(|s| s.location == *location).cloned().unwrap_or_default();
            out.push(VertexInput { location: *location, semantic: s.semantic, name: s.name, format: crate::formats::vertex_format(inner) });
        }
        (None, TypeInner::Struct { members, .. }) => {
            for m in members {
                collect_inputs(module, m.ty, m.binding.as_ref(), semantics, out);
            }
        }
        _ => {}
    }
}

/// Bind-group layout entries for a pipeline made of `stages`, indexed by group: entries used by
/// several stages are merged (visibility ORed). Missing groups are empty. Runtime-sized arrays
/// get `runtime_array_len` entries (wgpu needs a bound; the device limit decides it).
pub fn bind_group_layouts(stages: &[&Reflection], runtime_array_len: u32) -> Vec<Vec<wgt::BindGroupLayoutEntry>> {
    let mut groups: Vec<Vec<wgt::BindGroupLayoutEntry>> = Vec::new();
    for r in stages {
        for b in &r.bindings {
            let g = b.group as usize;
            if groups.len() <= g {
                groups.resize(g + 1, Vec::new());
            }
            match groups[g].iter_mut().find(|e| e.binding == b.binding) {
                Some(e) => e.visibility |= shader_stages(r.stage),
                None => groups[g].push(wgt::BindGroupLayoutEntry {
                    binding: b.binding,
                    visibility: shader_stages(r.stage),
                    ty: b.ty,
                    count: b.count.and_then(|n| NonZeroU32::new(if n == 0 { runtime_array_len } else { n })),
                }),
            }
        }
    }
    for g in &mut groups {
        g.sort_by_key(|e| e.binding);
    }
    groups
}
