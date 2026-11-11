//! A model's meshes as [`Draw`]s: every draw call of every mesh in the enabled mesh groups at
//! LOD 0, with the object's transform and tint, its morph weights (morph.rs) and bones
//! (render/skin).

use super::morph::{self, ObjectMorphs};
use super::{Draw, gpu_mesh, shaded, topology};
use crate::mesh::buffers::VertexBuffer;
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::{DrawCall, RenderMesh};
use crate::model::Model;
use crate::render::shading::bind::Instance;
use crate::render::skin;
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
    // Placeholders cull like native's default material, or an error cube around the eye fills the frame.
    let state = if shaded.is_none() { RasterState { cull: Some(render::wgpu::Face::Back), ..state } } else { state };
    Some(Draw { vertices, vertex_offset: 0, vertex, indices, range, base_vertex: 0, instances: 1, uniforms, state, shaded })
}

/// What shapes an object's draws, read before its state is locked: morph weights and, for an
/// animatable object, its bones; an aggregate's fragments; a dynamic object's own vertices.
pub struct Deform {
    key: usize,
    morphs: Option<ObjectMorphs>,
    pose: Option<skin::native::Pose>,
    fragments: Option<Arc<[crate::scene::aggregate::Fragment]>>,
    dynamic: Option<crate::scene::object::Ref>,
}

impl Deform {
    /// Locks the object's state: call it before taking that lock.
    pub fn of(r: crate::scene::object::Ref) -> Deform {
        let dynamic = crate::scene::dynamic::has_vertices(r).then_some(r);
        Deform { key: r.addr(), morphs: morph::of(r), pose: skin::native::pose(r), fragments: crate::scene::aggregate::fragments(r), dynamic }
    }

    /// Dynamic objects (debug overlays, gizmos) stay out of shadow maps.
    pub fn casts_shadow(&self) -> bool {
        self.dynamic.is_none()
    }
}

/// A scene object's draws, seen through `view_proj`, deformed by `deform`; none without a model
/// or vertices. Skinned meshes draw from the vertex cache (render/skin), the rest morph on the CPU.
pub fn object(s: &crate::scene::object::State, deform: &Deform, view_proj: Mat4) -> Vec<Draw> {
    if let Some(f) = &deform.fragments {
        return super::dynamic::aggregate(f, s, view_proj);
    }
    if let Some(r) = deform.dynamic {
        return super::dynamic::dynamic(r, s, view_proj).into_iter().collect();
    }
    let Some(model) = crate::model::get(s.model) else { return Vec::new() };
    let mut out = Vec::new();
    each_call(&model, s.mesh_groups, s.material_override.as_ref(), |i, mesh, call| {
        let world = matrix(&s.transform);
        let uniforms = DrawUniforms { mvp: view_proj * world, model: world, tint: tint(s.tint) };
        let posed = deform.pose.as_ref().and_then(|p| Some((p, skin::mesh_skin(&model, s.model, i)?)));
        let skinned = posed.and_then(|(pose, sk)| skin::native::skinned(deform.key, i, &sk, s.model, pose, tint(s.tint), deform.morphs.as_ref().and_then(|m| Some((m.mesh(i)?, mesh)))));
        let Some(attrs) = skinned else {
            let vertices = deform.morphs.as_ref().and_then(|m| morph::vertices(m, i, mesh));
            out.extend(draw_call_of(mesh, vertices, call, uniforms, view_proj, RasterState::default()));
            return;
        };
        let mut draw = draw_call_of(mesh, None, call, DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, ..uniforms }, view_proj, RasterState::default());
        if let Some(shaded) = draw.as_mut().and_then(|d| d.shaded.as_mut()) {
            shaded.context.attributes.push(attrs);
        }
        out.extend(draw);
    });
    out
}

/// `d` for a depth-only pass: the placeholder, unless it draws from the vertex cache, which
/// only its material program reads.
pub fn depth_only(d: Draw) -> Draw {
    let cached = d.shaded.as_ref().is_some_and(|s| !s.context.attributes.is_empty());
    if cached { d } else { Draw { shaded: None, ..d } }
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

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
