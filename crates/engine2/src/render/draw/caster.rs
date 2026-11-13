//! Shadow casters as native's shadow views draw them: through the depth-only program
//! (`shading/standin_depth.shader`, real's closed `depth_only.shader`), which binds the shadow
//! view's 1088-byte per-view constants and reads world transforms from `g_TransformBuffer` like
//! the main view's programs. Every rigid draw of the same mesh range becomes one instanced draw;
//! vertex-cache (skinned) draws keep their own bindings and switch program ([`on_depth`]).

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

/// A vertex-cache draw's depth program (its `D_CS_VERTEX_ANIMATION` variant, from the draw's
/// attributes) and vertex inputs from its mesh layout. It keeps the draw's own transform buffer: a
/// caster instance's would hold no cache offset.
fn on_depth(s: &super::shaded::Shaded) -> Option<(Arc<Shading>, Vec<VertexBufferKey>)> {
    let depth = shading::program(device::gpu()?, standin::DEPTH_PATH, &s.context.attributes)?;
    let keys = vertex::inputs(&depth.vertex_inputs, s.cached_mesh.as_ref()?)?.keys;
    Some((depth, keys))
}

/// `draws` seen through `view_proj` as casters: same-mesh rigid draws merged into instanced
/// draws, vertex-cache draws on the depth program, in real's opaque order ([`sort_opaque`]).
pub fn instanced(draws: Vec<Draw>, view_proj: Mat4) -> Vec<Draw> {
    let Some(depth) = program() else { return draws };
    let mut groups: Vec<(Draw, Vec<VertexBufferKey>, Vec<Instance>)> = Vec::new();
    let mut out = Vec::new();
    for mut d in draws {
        let keys = d.vertex.first().and_then(|k| vertex_keys(&depth, k));
        let cached = vertex_cached(&d).then(|| d.shaded.as_ref().and_then(on_depth)).flatten();
        match (d.shaded.take(), keys, cached) {
            (Some(s), _, Some((shading, vertex))) => out.push(Draw { shaded: Some(super::shaded::Shaded { shading, vertex, ..s }), ..d }),
            (None, Some(keys), _) => {
                let instance = Instance { world: d.uniforms.model, tint: d.uniforms.tint };
                match groups.iter_mut().find(|(g, _, _)| same_mesh(g, &d)) {
                    Some((g, _, instances)) => {
                        instances.push(instance);
                        g.cull.bounds.append(&mut d.cull.bounds);
                    }
                    None => groups.push((d, keys, vec![instance])),
                }
            }
            (shaded, _, _) => out.push(Draw { shaded, ..d }),
        }
    }
    let uniforms = |d: &Draw| render::pass::DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, ..d.uniforms };
    let batch = |(d, vertex, instances): (Draw, Vec<VertexBufferKey>, Vec<Instance>)| Draw { instances: instances.len() as u32, uniforms: uniforms(&d), vertex, caster: Some(instances), ..d };
    let mut all: Vec<Draw> = groups.into_iter().map(batch).collect();
    all.extend(out);
    sort_opaque(&mut all);
    all
}

/// A caster batch's bind groups for the depth program, its pipeline for `key` built.
pub fn bind(gpu: &render::Gpu, depth: &Shading, instances: &[Instance], view: &View, key: &PipelineKey, targets: &[texture::Ref]) -> Option<Vec<wgpu::BindGroup>> {
    let lighting = Lighting::default();
    let none = HashMap::new();
    let inputs = binding::Inputs { view, lighting: &lighting, instances, textures: &HashMap::new(), values: &[&none], attributes: &[], slots: &[], targets };
    let built = gpu.checked(|| binding::bind_groups(gpu, depth, &inputs)).and_then(|groups| groups);
    built.and_then(|groups| depth.program.try_pipeline(gpu, key).map(|_| groups)).inspect_err(|e| super::shaded::warn_once(&depth.label, e)).ok()
}
