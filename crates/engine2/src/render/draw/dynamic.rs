//! Draws of objects whose geometry isn't a model: dynamic objects' vertices (scene/dynamic.rs)
//! and aggregates' fragments (scene/aggregate.rs).

use super::Draw;
use super::model::{self as model_draw, matrix, tint};
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::DrawCall;
use crate::scene::{aggregate, dynamic, object};
use glam::Mat4;
use render::RasterState;
use render::pass::DrawUniforms;

/// A dynamic object's one draw: all its vertices as its primitive type, in its material.
pub fn dynamic(r: object::Ref, s: &object::State, view_proj: Mat4) -> Option<Draw> {
    let (mesh, prim) = dynamic::mesh(r)?;
    let world = matrix(&s.transform);
    let uniforms = DrawUniforms { mvp: view_proj * world, model: world, tint: tint(s.tint) };
    let n = mesh.state().index_range.1;
    let call = DrawCall { material: MaterialRef::keep(dynamic::material(r)), prim: prim as i32, start_index: 0, index_count: n, start_vertex: 0, vertex_count: n };
    model_draw::draw_call(&mesh, &call, uniforms, view_proj, RasterState::default())
}

/// An aggregate's fragments: each its own draw call of the model's first mesh, placed and tinted.
pub fn aggregate(fragments: &[aggregate::Fragment], s: &object::State, view_proj: Mat4) -> Vec<Draw> {
    let Some(model) = crate::model::get(s.model) else { return Vec::new() };
    let Some(mesh) = model.content.meshes.first().map(|m| m.mesh.clone()) else { return Vec::new() };
    let calls = mesh.draw_calls();
    let object = matrix(&s.transform);
    let base = tint(s.tint);
    fragments
        .iter()
        .filter_map(|f| {
            let call = calls.get(usize::try_from(f.draw).ok()?)?;
            let world = object * matrix(&f.transform);
            let t = tint(f.tint);
            let uniforms = DrawUniforms { mvp: view_proj * world, model: world, tint: std::array::from_fn(|i| base[i] * t[i]) };
            model_draw::draw_call(&mesh, call, uniforms, view_proj, RasterState::default())
        })
        .collect()
}
