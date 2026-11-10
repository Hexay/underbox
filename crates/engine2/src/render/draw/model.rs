//! A model's meshes as [`Draw`]s: every draw call of every mesh in the enabled mesh groups at
//! LOD 0, with the object's transform and tint.

use super::{Draw, gpu_mesh, shaded, topology};
use crate::mesh::material_ref::MaterialRef;
use std::sync::Arc;
use crate::render::shading::bind::Instance;
use crate::mesh::render_mesh::{DrawCall, RenderMesh};
use crate::model::Model;
use abi::structs::{Transform, Vector4};
use glam::{Mat4, Quat, Vec3};
use render::RasterState;
use render::pass::DrawUniforms;

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
    let (vb, ib) = {
        let s = mesh.state();
        (s.vertices.clone()?, s.indices.clone())
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
    let shaded = call.material.as_ref().and_then(|m| m.material()).and_then(|m| shaded::resolve(m, &vb.meta, instance, view_proj));
    Some(Draw { vertices, vertex_offset: 0, vertex, indices, range, base_vertex: 0, instances: 1, uniforms, state, shaded })
}

/// Every draw of `model` placed at `world` (model to world), seen through `view_proj`, each with
/// its own material unless `material` overrides them all.
pub fn draws(model: &Model, world: Mat4, view_proj: Mat4, tint: [f32; 4], mesh_groups: u64, material: Option<&Arc<MaterialRef>>) -> Vec<Draw> {
    let uniforms = DrawUniforms { mvp: view_proj * world, model: world, tint };
    let mut out = Vec::new();
    for m in &model.content.meshes {
        if m.lod_mask & 1 == 0 || (m.group_mask != 0 && m.group_mask & mesh_groups == 0) {
            continue;
        }
        for mut call in m.mesh.draw_calls() {
            if let Some(material) = material {
                call.material = Some(material.clone());
            }
            out.extend(draw_call(&m.mesh, &call, uniforms, view_proj, RasterState::default()));
        }
    }
    out
}
