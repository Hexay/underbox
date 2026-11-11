//! `RenderTools.DrawModelDrawCall*`, what `Sandbox.SceneRenderer` (`r_managed_scene`) draws with:
//! one draw call of one mesh of a model in a given material, instances whose ids come from
//! managed's instance-id buffer and whose transforms come from the `g_TransformBuffer` attribute
//! (shading/bind.rs). The shader mode token is not consulted: a material has one program.
//!
//! The indirect forms read their `IndirectDrawIndexedArguments` back (the occlusion cull wrote
//! them, compute/occlusion.rs) and draw what they say; the multi form's args carry their own
//! index ranges, absolute in the mesh's index buffer like the draw call's own.
//!
//! Vertex-cache draws (render/skin) go the long way: `BindModelDrawCall` +
//! `SetRenderStateForModelDrawCall` record a [`ModelCall`] on the context, stream 1 carries the
//! instance ids, and the context's `DrawIndexedInstanced` draws it ([`draw_bound_call`]).
//! `BindModelDrawCallVertexIds` binds nothing: the stand-in finds cached vertices by vertex index.

use super::shaded::{self, ContextBindings};
use super::{immediate, model as model_draw};
use crate::render::material::Material;
use crate::render::shading::bind::Instance;
use crate::render::{attributes, buffers as gpu_buffers, context, device, material, material_mode};
use abi::slots::render_tools as tools;
use abi::{Registry, bind};
use glam::Mat4;
use render::RasterState;
use render::pass::DrawUniforms;
use render::wgpu::CompareFunction;
use std::ops::Range;
use std::sync::Arc;

/// One draw of the call: `count` instances from id slot `first`, over the call's own index range
/// or `range` (indices, base vertex).
struct Sub {
    first: u32,
    count: u32,
    range: Option<(Range<u32>, i32)>,
}

/// Bytes of one `IndirectDrawIndexedArguments`.
const ARGS: u64 = 20;

/// Up to `bytes` of `b` from byte `offset`, as words.
fn read_words(b: gpu_buffers::Handle, offset: i32, bytes: u64) -> Vec<u32> {
    let (Some(gpu), Some(buf)) = (device::gpu(), gpu_buffers::get(b)) else { return Vec::new() };
    let offset = u64::from(offset.max(0).unsigned_abs());
    let len = bytes.min(buf.size().saturating_sub(offset));
    crate::render::compute::words(&buf.read(gpu, offset, len))
}

/// `(index count, instance count, first index, base vertex, first instance)` × `count` at byte `offset`.
fn read_args(b: gpu_buffers::Handle, offset: i32, count: u32) -> Vec<[u32; 5]> {
    read_words(b, offset, u64::from(count) * ARGS).as_chunks::<5>().0.to_vec()
}

/// Native's `RsDepthBiasStateOverride_t` from managed's shadow passes: constant and slope scale
/// (negative for reverse-Z maps).
type Bias = (i32, f32);

/// A model draw call bound on a context (`BindModelDrawCall`), with the render state
/// `SetRenderStateForModelDrawCall` gave it.
#[derive(Clone, Copy)]
pub struct ModelCall {
    attrs: attributes::Ref,
    mode: Option<material_mode::Ref>,
    model: crate::model::Ref,
    call: (i32, i32),
    bias: Bias,
}

#[allow(clippy::too_many_arguments)]
fn draw_model(c: context::Ref, attrs: attributes::Ref, material: Arc<Material>, m: crate::model::Ref, (mesh, call): (i32, i32), bias: Bias, ids: gpu_buffers::Handle, subs: &[Sub]) -> bool {
    let Some(model) = crate::model::get(m) else { return false };
    let Some(entry) = usize::try_from(mesh).ok().and_then(|i| model.content.meshes.get(i)) else { return false };
    let Some(mut dc) = usize::try_from(call).ok().and_then(|i| entry.mesh.draw_calls().into_iter().nth(i)) else { return false };
    let Some(layout) = entry.mesh.state().vertices.as_ref().map(|v| v.meta.clone()) else { return false };
    dc.material = None;
    let view_proj = immediate::view_proj(c);
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let depth_compare = if context::reverse_z(c) { CompareFunction::GreaterEqual } else { CompareFunction::LessEqual };
    let raster = RasterState { depth_compare, depth_bias: (bias.0, bias.1.to_bits()), ..Default::default() };
    let ids = gpu_buffers::get(ids);
    let mut draws = Vec::new();
    for sub in subs.iter().filter(|s| s.count > 0) {
        let bindings = ContextBindings {
            attributes: vec![attrs, context::get(c).map(|x| x.attributes).unwrap_or_default()],
            slots: immediate::with(c, |s| s.slots.clone()),
            instance_ids: ids.clone().map(|b| (b, u64::from(sub.first) * 4)),
        };
        // The placeholder can't place instances without their transforms: no program, no draw.
        let Some(shaded) = shaded::resolve(material.clone(), &layout, Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }, view_proj, bindings) else { return false };
        if crate::render::scene::trace::enabled() {
            trace(c, &shaded.context, sub.count);
        }
        let Some(mut draw) = model_draw::draw_call(&entry.mesh, &dc, uniforms, view_proj, raster) else { return false };
        (draw.shaded, draw.instances) = (Some(shaded), sub.count);
        if let Some((range, base)) = &sub.range {
            (draw.range, draw.base_vertex) = (range.clone(), *base);
        }
        draws.push(draw);
    }
    immediate::submit(c, &draws);
    true
}

/// `UNDERBOX_LAYERS=log`: read back the ids and transforms this draw will use.
fn trace(c: context::Ref, b: &ContextBindings, n: u32) {
    let Some(gpu) = device::gpu() else { return };
    let ids: Vec<u32> = b.instance_ids.as_ref().map_or_else(Vec::new, |(buf, offset)| {
        let len = (u64::from(n) * 4).min(buf.size().saturating_sub(*offset));
        crate::render::compute::words(&buf.read(gpu, *offset, len))
    });
    let transforms = b.attributes.iter().find_map(|a| attributes::buffer(*a, "g_TransformBuffer")).and_then(|h| gpu_buffers::get(gpu_buffers::Handle::new(h)));
    let transforms = transforms.map(|t| t.read(gpu, 0, t.size()));
    crate::render::scene::trace::draw_call(&context::target(c), context::reverse_z(c), &ids, transforms.as_deref());
}

fn slot(first: i32) -> u32 {
    first.max(0).unsigned_abs()
}

/// `DrawModelDrawCallIndirect`: the instance count from the args at byte `offset` of `args`.
#[allow(clippy::too_many_arguments)]
fn draw_indirect(c: context::Ref, attrs: attributes::Ref, mat: material::Ref, m: crate::model::Ref, call: (i32, i32), bias: Bias, (ids, first): (gpu_buffers::Handle, i32), (args, offset): (gpu_buffers::Handle, i32)) -> bool {
    let count = read_args(args, offset, 1).first().map_or(0, |a| a[1]);
    material::resolve(mat).is_some_and(|mat| draw_model(c, attrs, mat, m, call, bias, ids, &[Sub { first: slot(first), count, range: None }]))
}

fn bind_call(c: context::Ref, attrs: attributes::Ref, m: crate::model::Ref, mesh: i32, call: i32) -> bool {
    let valid = crate::model::get(m).zip(usize::try_from(mesh).ok()).is_some_and(|(model, i)| model.content.meshes.get(i).is_some_and(|e| usize::try_from(call).is_ok_and(|c| c < e.mesh.draw_calls().len())));
    immediate::with(c, |s| s.model_call = valid.then_some(ModelCall { attrs, mode: None, model: m, call: (mesh, call), bias: (0, 0.0) }));
    valid
}

fn set_call_state(c: context::Ref, attrs: attributes::Ref, mode: material_mode::Ref, m: crate::model::Ref, call: (i32, i32), bias: Bias) -> bool {
    let ok = material_mode::material(mode).is_some();
    immediate::with(c, |s| s.model_call = ok.then_some(ModelCall { attrs, mode: Some(mode), model: m, call, bias }));
    ok
}

/// The context's `DrawIndexedInstanced` of a bound [`ModelCall`]: `n` instances from the ids on
/// stream 1. False when no model call with render state is bound.
pub(crate) fn draw_bound_call(c: context::Ref, n: i32) -> bool {
    let Some((call, (ids, offset))) = immediate::with(c, |s| s.model_call.zip(s.instance_stream)) else { return false };
    let Some(material) = call.mode.and_then(material_mode::material) else { return false };
    draw_model(c, call.attrs, material, call.model, call.call, call.bias, ids, &[Sub { first: offset / 4, count: slot(n), range: None }])
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::DRAW_MODEL_DRAW_CALL, |c, attrs, mat, _mode, m, mesh, call, bias, slope, _strip, _stencil, _skinned, ids, first, n, _bind| {
        let subs = [Sub { first: slot(first), count: n.max(1).unsigned_abs(), range: None }];
        material::resolve(mat).is_some_and(|mat| draw_model(c, attrs, mat, m, (mesh, call), (bias, slope), ids, &subs))
    });
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_INDIRECT, |c, attrs, mat, _mode, m, mesh, call, bias, slope, _strip, _stencil, _skinned, ids, first, args, offset, _bind| {
        draw_indirect(c, attrs, mat, m, (mesh, call), (bias, slope), (ids, first), (args, offset))
    });
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_MULTI_INDIRECT, |c, attrs, mat, _mode, m, mesh, call, bias, slope, _strip, _stencil, ids, first, args, offset, count, counts, count_offset, _bind| {
        // A count buffer's first word caps the draws (drawIndirectCount).
        let count = if counts.value == 0 { slot(count) } else { read_words(counts, count_offset, 4).first().map_or(0, |n| (*n).min(slot(count))) };
        let subs: Vec<Sub> = read_args(args, offset, count)
            .iter()
            .map(|a| Sub { first: slot(first) + a[4], count: a[1], range: Some((a[2]..a[2] + a[0], a[3] as i32)) })
            .collect();
        material::resolve(mat).is_some_and(|mat| draw_model(c, attrs, mat, m, (mesh, call), (bias, slope), ids, &subs))
    });
    bind!(r, tools::BIND_MODEL_DRAW_CALL, bind_call);
    bind!(r, tools::SET_RENDER_STATE_FOR_MODEL_DRAW_CALL, |c, attrs, mode, m, mesh, call, bias, slope, _strip, _cache, _stencil| {
        set_call_state(c, attrs, mode, m, (mesh, call), (bias, slope))
    });
    bind!(r, tools::BIND_MODEL_DRAW_CALL_VERTEX_IDS, |c, _m, _mesh, _call, _stream| immediate::with(c, |s| s.model_call.is_some()));
}

#[cfg(test)]
#[path = "draw_call_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "managed_shadow_tests.rs"]
mod managed_shadow_tests;
