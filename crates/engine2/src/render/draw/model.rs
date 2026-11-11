//! A model's meshes as [`Draw`]s: every draw call of every mesh in the enabled mesh groups at
//! LOD 0, with the object's transform and tint, and its morph weights (morph.rs).

use super::morph::{self, ObjectMorphs};
use super::{Draw, gpu_mesh, shaded, topology};
use crate::mesh::buffers::VertexBuffer;
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::{DrawCall, RenderMesh};
use crate::model::Model;
use crate::render::shading::bind::Instance;
use abi::structs::{Transform, Vector4};
use glam::{Mat4, Quat, Vec3};
use render::RasterState;
use render::pass::DrawUniforms;
use std::sync::Arc;

pub fn matrix(t: &Transform) -> Mat4 {
    let s = t.scale.vec;
    // A zeroed scale (managed default structs) would collapse the mesh; treat it as unit scale.
    let scale = if s.x == 0.0 && s.y == 0.0 && s.z == 0.0 { Vec3::ONE } else { Vec3::new(s.x, s.y, s.z) };
    let q = t.rotation.quat;
    let rot = Quat::from_xyzw(q.x, q.y, q.z, q.w);
    let rot = if rot.length_squared() > 0.0 { rot.normalize() } else { Quat::IDENTITY };
    Mat4::from_scale_rotation_translation(scale, rot, Vec3::new(t.position.vec.x, t.position.vec.y, t.position.vec.z))
}

pub fn tint(v: Vector4) -> [f32; 4] {
    [v.vec.x, v.vec.y, v.vec.z, v.vec.w]
}

/// One mesh draw call, if its buffers upload and its layout has a position. `view_proj` is
/// what `uniforms.mvp` was made from; the material program takes the two apart.
pub fn draw_call(mesh: &RenderMesh, call: &DrawCall, uniforms: DrawUniforms, view_proj: Mat4, state: RasterState) -> Option<Draw> {
    draw_call_of(mesh, None, call, uniforms, view_proj, state)
}

/// [`draw_call`] from `vertices` in place of the mesh's own vertex buffer, when given.
fn draw_call_of(mesh: &RenderMesh, vertices: Option<Arc<VertexBuffer>>, call: &DrawCall, uniforms: DrawUniforms, view_proj: Mat4, state: RasterState) -> Option<Draw> {
    let (vb, ib) = {
        let s = mesh.state();
        (vertices.or_else(|| s.vertices.clone())?, s.indices.clone())
    };
    let vertex = gpu_mesh::vertex_keys(&vb.meta)?;
    let vertices = gpu_mesh::vertices(&vb)?;
    let indices = ib.and_then(|ib| gpu_mesh::indices(&ib));
    let start = |v: i32| v.max(0) as u32;
    let range = match indices {
        Some(_) => start(call.start_index)..start(call.start_index) + start(call.index_count),
        None => start(call.start_vertex)..start(call.start_vertex) + start(call.vertex_count),
    };
    if range.is_empty() {
        return None;
    }
    let state = RasterState { topology: topology(call.prim.into()), ..state };
    let instance = Instance { world: uniforms.model, tint: uniforms.tint };
    let shaded = call.material.as_ref().and_then(|m| m.material()).and_then(|m| shaded::resolve(m, &vb.meta, instance, view_proj, Default::default()));
    Some(Draw { vertices, vertex_offset: 0, vertex, indices, range, base_vertex: 0, instances: 1, uniforms, state, shaded })
}

/// A scene object's model draws, seen through `view_proj`, morphed by `morphs` (from
/// [`morph::of`]); none without a model.
pub fn object(s: &crate::scene::object::State, morphs: Option<&ObjectMorphs>, view_proj: Mat4) -> Vec<Draw> {
    let Some(model) = crate::model::get(s.model) else { return Vec::new() };
    let mut out = Vec::new();
    each_call(&model, s.mesh_groups, s.material_override.as_ref(), |i, mesh, call| {
        let vertices = morphs.and_then(|m| morph::vertices(m, i, mesh));
        let world = matrix(&s.transform);
        out.extend(draw_call_of(mesh, vertices, call, DrawUniforms { mvp: view_proj * world, model: world, tint: tint(s.tint) }, view_proj, RasterState::default()));
    });
    out
}

/// Every draw of `model` placed at `world` (model to world), seen through `view_proj`, each with
/// its own material unless `material` overrides them all.
pub fn draws(model: &Model, world: Mat4, view_proj: Mat4, tint: [f32; 4], mesh_groups: u64, material: Option<&Arc<MaterialRef>>) -> Vec<Draw> {
    let uniforms = DrawUniforms { mvp: view_proj * world, model: world, tint };
    let mut out = Vec::new();
    each_call(model, mesh_groups, material, |_, mesh, call| out.extend(draw_call(mesh, call, uniforms, view_proj, RasterState::default())));
    out
}

/// `f(mesh index, mesh, call)` for each draw call of the LOD 0 meshes in `mesh_groups`, with
/// `material` overriding the calls' own.
fn each_call(model: &Model, mesh_groups: u64, material: Option<&Arc<MaterialRef>>, mut f: impl FnMut(usize, &RenderMesh, &DrawCall)) {
    for (i, m) in model.content.meshes.iter().enumerate() {
        if m.lod_mask & 1 == 0 || (m.group_mask != 0 && m.group_mask & mesh_groups == 0) {
            continue;
        }
        for mut call in m.mesh.draw_calls() {
            if let Some(material) = material {
                call.material = Some(material.clone());
            }
            f(i, &m.mesh, &call);
        }
    }
}
