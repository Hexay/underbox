//! Shadow casters as native's shadow views draw them: through the depth-only program
//! (`shading/standin_depth.shader`, real's closed `depth_only.shader`), which binds the shadow
//! view's 1088-byte per-view constants and reads world transforms from `g_TransformBuffer` like
//! the main view's programs. Every rigid draw of the same mesh range becomes one instanced draw;
//! vertex-cache (skinned) draws keep their own bindings and switch program.

use super::{Draw, device, sort_opaque, vertex_cached};
use crate::mesh::layout::{Field, Layout, format};
use crate::render::shading::bind::{self as binding, Instance};
use crate::render::shading::{self, Shading, standin, vertex, view::View};
use crate::render::lighting::Lighting;
use crate::render::texture;
use glam::Mat4;
use render::wgpu::{self, VertexFormat};
use render::{PipelineKey, VertexBufferKey};
use std::collections::HashMap;
use std::sync::Arc;

pub fn program() -> Option<Arc<Shading>> {
    shading::program(device::gpu()?, standin::DEPTH_PATH, &[])
}

/// What makes two draws one mesh range: buffers by identity, offsets, range, layout and state.
fn same_mesh(a: &Draw, b: &Draw) -> bool {
    let indices = |d: &Draw| d.indices.as_ref().map(|(b, f)| (Arc::as_ptr(b), *f));
    Arc::ptr_eq(&a.vertices, &b.vertices)
        && a.vertex_offset == b.vertex_offset
        && indices(a) == indices(b)
        && a.range == b.range
        && a.base_vertex == b.base_vertex
        && a.state == b.state
        && a.vertex.first() == b.vertex.first()
}

/// The depth program's vertex inputs from a placeholder mesh key (its position at location 0).
fn vertex_keys(depth: &Shading, mesh: &VertexBufferKey) -> Option<Vec<VertexBufferKey>> {
    let position = mesh.attributes.iter().find(|a| a.shader_location == 0)?;
    let format = match position.format {
        VertexFormat::Float32x4 => format::RGBA32F,
        VertexFormat::Float32x3 => format::RGB32F,
        VertexFormat::Float32x2 => format::RG32F,
        _ => return None,
    };
    let field = Field { semantic: "position".into(), index: 0, format, offset: position.offset as usize };
    let layout = Layout { name: "shadow caster".into(), stride: mesh.stride as usize, fields: vec![field] };
    vertex::inputs(&depth.vertex_inputs, &layout).map(|i| i.keys)
}

/// What a draw becomes on the depth program.
enum OnDepth {
    /// A rigid draw: an instance of a caster batch with these vertex inputs.
    Instance(Box<Draw>, Vec<VertexBufferKey>, Instance),
    /// A vertex-cache draw already switched to the depth program, or one it can't take.
    Done(Box<Draw>),
}

/// A vertex-cache draw keeps its bindings and switches program; a placeholder draw becomes a
/// transform-buffer instance; any other stays as it is.
fn on_depth(depth: &Arc<Shading>, mut d: Draw) -> OnDepth {
    let keys = d.vertex.first().and_then(|k| vertex_keys(depth, k));
    let cached = vertex_cached(&d);
    match (d.shaded.take(), keys) {
        (Some(s), Some(keys)) if cached => OnDepth::Done(Box::new(Draw { shaded: Some(super::shaded::Shaded { shading: depth.clone(), vertex: keys, ..s }), ..d })),
        (None, Some(keys)) => {
            let instance = Instance { world: d.uniforms.model, tint: d.uniforms.tint };
            OnDepth::Instance(Box::new(d), keys, instance)
        }
        (shaded, _) => OnDepth::Done(Box::new(Draw { shaded, ..d })),
    }
}

fn batch(d: Draw, vertex: Vec<VertexBufferKey>, instances: Vec<Instance>, view_proj: Mat4) -> Draw {
    let uniforms = render::pass::DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, ..d.uniforms };
    Draw { instances: instances.len() as u32, uniforms, vertex, caster: Some(instances), ..d }
}

/// `draws` seen through `view_proj` as casters: same-mesh rigid draws merged into instanced
/// draws, vertex-cache draws on the depth program, in real's opaque order ([`sort_opaque`]).
pub fn instanced(draws: Vec<Draw>, view_proj: Mat4) -> Vec<Draw> {
    let Some(depth) = program() else { return draws };
    let mut groups: Vec<(Draw, Vec<VertexBufferKey>, Vec<Instance>)> = Vec::new();
    let mut out = Vec::new();
    for d in draws {
        match on_depth(&depth, d) {
            OnDepth::Instance(d, keys, instance) => match groups.iter_mut().find(|(g, _, _)| same_mesh(g, &d)) {
                Some((_, _, instances)) => instances.push(instance),
                None => groups.push((*d, keys, vec![instance])),
            },
            OnDepth::Done(d) => out.push(*d),
        }
    }
    let mut all: Vec<Draw> = groups.into_iter().map(|(d, vertex, instances)| batch(d, vertex, instances, view_proj)).collect();
    all.extend(out);
    sort_opaque(&mut all);
    all
}

/// A rigid material draw on the depth program as a transform-buffer instance of its own, for the
/// main view's depth prepass, which binds no material textures (real's binds none). Vertex-cache
/// draws keep their material program: on the depth program their prepass depth comes out in rest
/// pose (skinned's open crates). Placeholder draws bind no material textures as they are.
pub fn single(d: Draw, view_proj: Mat4) -> Draw {
    let Some(depth) = program() else { return d };
    if d.shaded.is_none() || vertex_cached(&d) {
        return d;
    }
    match d.vertex.first().and_then(|k| vertex_keys(&depth, k)) {
        Some(keys) => {
            let instance = Instance { world: d.uniforms.model, tint: d.uniforms.tint };
            batch(Draw { shaded: None, ..d }, keys, vec![instance], view_proj)
        }
        None => d,
    }
}

/// A caster batch's bind groups for the depth program, its pipeline for `key` built.
pub fn bind(gpu: &render::Gpu, depth: &Shading, instances: &[Instance], view: &View, key: &PipelineKey, targets: &[texture::Ref]) -> Option<Vec<wgpu::BindGroup>> {
    let lighting = Lighting::default();
    let none = HashMap::new();
    let inputs = binding::Inputs { view, lighting: &lighting, instances, textures: &HashMap::new(), values: &[&none], attributes: &[], slots: &[], targets };
    let built = gpu.checked(|| binding::bind_groups(gpu, depth, &inputs)).and_then(|groups| groups);
    built.and_then(|groups| depth.program.try_pipeline(gpu, key).map(|_| groups)).inspect_err(|e| super::shaded::warn_once(&depth.label, e)).ok()
}
