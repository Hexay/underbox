//! `RenderTools.DrawModelDrawCall`, what `Sandbox.SceneRenderer` (`r_managed_scene`) draws with:
//! one draw call of one mesh of a model in a given material, `n` instances whose ids come from
//! managed's instance-id buffer (from slot `first`) and whose transforms come from the
//! `g_TransformBuffer` attribute (shading/bind.rs). The shader mode token is not consulted: a
//! material has one program.

use super::shaded::{self, ContextBindings};
use super::{immediate, model as model_draw};
use crate::render::shading::bind::Instance;
use crate::render::{attributes, buffers as gpu_buffers, context, material};
use abi::slots::render_tools as tools;
use abi::{Registry, bind};
use glam::Mat4;
use render::RasterState;
use render::wgpu::CompareFunction;
use render::pass::DrawUniforms;

#[allow(clippy::too_many_arguments)]
fn draw_model_draw_call(c: context::Ref, attrs: attributes::Ref, mat: material::Ref, m: crate::model::Ref, mesh: i32, call: i32, ids: gpu_buffers::Handle, first: i32, n: i32) -> bool {
    let (Some(model), Some(material)) = (crate::model::get(m), material::resolve(mat)) else { return false };
    let Some(entry) = usize::try_from(mesh).ok().and_then(|i| model.content.meshes.get(i)) else { return false };
    let Some(mut dc) = usize::try_from(call).ok().and_then(|i| entry.mesh.draw_calls().into_iter().nth(i)) else { return false };
    let Some(layout) = entry.mesh.state().vertices.as_ref().map(|v| v.meta.clone()) else { return false };
    dc.material = None;
    let view_proj = immediate::view_proj(c);
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let depth_compare = if context::reverse_z(c) { CompareFunction::GreaterEqual } else { CompareFunction::LessEqual };
    let raster = RasterState { depth_compare, ..Default::default() };
    let Some(mut draw) = model_draw::draw_call(&entry.mesh, &dc, uniforms, view_proj, raster) else { return false };
    let bindings = ContextBindings {
        attributes: vec![attrs, context::get(c).map(|x| x.attributes).unwrap_or_default()],
        slots: immediate::with(c, |s| s.slots.clone()),
        instance_ids: gpu_buffers::get(ids).map(|b| (b, u64::from(first.max(0).unsigned_abs()) * 4)),
    };
    // The placeholder can't place instances without their transforms: no program, no draw.
    let Some(shaded) = shaded::resolve(material, &layout, Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }, view_proj, bindings) else { return false };
    draw.shaded = Some(shaded);
    draw.instances = n.max(1).unsigned_abs();
    immediate::submit(c, &[draw]);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::DRAW_MODEL_DRAW_CALL, |c, attrs, mat, _mode, m, mesh, call, _bias, _slope, _strip, _stencil, _skinned, ids, first, n, _bind| {
        draw_model_draw_call(c, attrs, mat, m, mesh, call, ids, first, n)
    });
    // TODO: the indirect forms take their instance counts from GPU buffers (draw_indirect).
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_INDIRECT, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_MULTI_INDIRECT, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
}
