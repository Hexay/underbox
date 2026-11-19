//! Draws whose arguments managed leaves in a GPU buffer, read there by `draw_indirect` /
//! `draw_indexed_indirect` (never read back): `IRenderContext.Draw*InstancedIndirect` from the
//! bound buffers, and `RenderTools.DrawModel`'s buffer forms.
//!
//! `DrawModel_1/3` (`Graphics.DrawModelInstancedIndirect(model, args)`) take only the instance
//! count from managed's args (byte 4 in both argument layouts): a GPU copy puts it into args of
//! our own, one per draw call of the LOD's meshes, every instance at the identity transform.
//! `DrawModel_4`'s args hold one `IndirectDrawIndexedArguments` per draw call of the LOD's first
//! mesh (`Model.GetLodDrawCallRange`), and its instances read their transforms from managed's
//! buffer as `g_TransformBuffer` (`GpuInstanceTransform`).

use super::shaded::ContextBindings;
use super::{Draw, Indirect, immediate, model as model_draw};
use crate::model::Model;
use crate::model::content::ModelMesh;
use crate::render::{attributes, buffers as gpu_buffers, context, device};
use abi::slots::{i_render_context as ctx, render_tools as tools};
use abi::{Registry, bind};
use glam::Mat4;
use render::pass::DrawUniforms;
use render::{BufferKind, RasterState};
use std::sync::Arc;

type Ctx = context::Ref;

/// Bytes of `IndirectDrawArguments` / `IndirectDrawIndexedArguments`.
const DRAW_ARGS: u64 = 16;
const INDEXED_ARGS: u64 = 20;
/// The instance count's byte offset in either layout.
const INSTANCE_COUNT: u64 = 4;

/// The buffer behind `h` when `size` bytes of arguments fit at `offset`, 4-byte aligned as wgpu
/// requires.
fn args_at(h: gpu_buffers::Handle, offset: u64, size: u64) -> Option<Arc<render::Buffer>> {
    gpu_buffers::get(h).filter(|b| offset.is_multiple_of(4) && offset + size <= b.size())
}

fn indirect(args: Arc<render::Buffer>, args_offset: u64) -> Option<Indirect> {
    Some(Indirect { args, args_offset, ids: None })
}

/// `count` draws of the bound buffers under `SetRenderState`, their args `stride` bytes apart
/// from `offset` (packed when 0).
fn draw_bound(c: Ctx, prim: i64, args: gpu_buffers::Handle, offset: u32, count: u32, stride: u32, indexed: bool) {
    let size = if indexed { INDEXED_ARGS } else { DRAW_ARGS };
    let stride = if stride == 0 { size } else { u64::from(stride) };
    let mut draws = Vec::new();
    for at in (0..u64::from(count)).map(|i| u64::from(offset) + i * stride) {
        let Some(buffer) = args_at(args, at, size) else { break };
        let Some((mut draw, layout)) = immediate::with(c, |s| immediate::bound_draw(s, prim, 0..0, 1, 0, indexed)) else { return };
        draw.shaded = immediate::render_state_shading(c, &layout);
        if (indexed && draw.indices.is_none()) || (draw.shaded.is_none() && draw.vertex.is_empty()) {
            return;
        }
        draw.cull.indirect = indirect(buffer, at);
        draws.push(draw);
    }
    immediate::submit(c, &draws);
}

/// The meshes drawn at `lod` (those carrying its bit), LOD 0's when none does.
fn lod_meshes(model: &Model, lod: i32) -> Vec<&ModelMesh> {
    let with = |bit: u8| model.content.meshes.iter().filter(|m| m.lod_mask & bit != 0).collect::<Vec<_>>();
    let meshes = with(1 << lod.clamp(0, 7));
    if meshes.is_empty() { with(1) } else { meshes }
}

/// Each draw call of `mesh` (`None` where one has nothing to draw), at the identity through the
/// context's view, shaded with `attributes` over the context's.
fn call_draws(c: Ctx, mesh: &ModelMesh, mut attributes: Vec<attributes::Ref>) -> Vec<Option<Draw>> {
    let view_proj = immediate::view_proj(c);
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    attributes.extend(context::get(c).map(|x| x.attributes));
    let bindings = ContextBindings { attributes, ..Default::default() };
    mesh.mesh.draw_calls().iter().map(|call| model_draw::draw_call_of(&mesh.mesh, None, call, uniforms, view_proj, RasterState::default(), bindings.clone())).collect()
}

/// `DrawModel_1/3`: every draw call of the LOD's meshes, as many instances as the args at `offset` of `args` say.
fn instanced(c: Ctx, m: crate::model::Ref, lod: i32, (args, offset): (gpu_buffers::Handle, i32), attrs: attributes::Ref) {
    let (Some(gpu), Some(model)) = (device::gpu(), crate::model::get(m)) else { return };
    let Some(source) = u64::try_from(offset).ok().and_then(|o| Some((args_at(args, o, INSTANCE_COUNT + 4)?, o))) else { return };
    let mut draws: Vec<Draw> = lod_meshes(&model, lod).into_iter().flat_map(|mesh| call_draws(c, mesh, vec![attrs])).flatten().collect();
    if draws.is_empty() {
        return;
    }
    let words = draws.iter().flat_map(|d| [d.range.len() as u32, 0, d.range.start, if d.indices.is_some() { d.base_vertex as u32 } else { 0 }, 0]);
    let bytes: Vec<u8> = words.flat_map(u32::to_le_bytes).collect();
    let ours = Arc::new(render::Buffer::with_data(gpu, "model indirect args", BufferKind::Storage, &bytes));
    for (i, d) in draws.iter_mut().enumerate() {
        let at = i as u64 * INDEXED_ARGS;
        source.0.copy_to(gpu, &ours, source.1 + INSTANCE_COUNT, at + INSTANCE_COUNT, 4);
        d.cull.indirect = indirect(ours.clone(), at);
    }
    immediate::submit(c, &draws);
}

/// `DrawModel_4`: each draw call `d` of the LOD's first mesh from the args at `offset + 20 d`,
/// its instances placed by `transforms`. Draws without a material program can't read them: none.
fn transformed(c: Ctx, m: crate::model::Ref, lod: i32, transforms: gpu_buffers::Handle, (args, offset): (gpu_buffers::Handle, i32), attrs: attributes::Ref) {
    let Some(model) = crate::model::get(m) else { return };
    let (Some(mesh), Ok(offset), Some(_)) = (lod_meshes(&model, lod).first().copied(), u64::try_from(offset), gpu_buffers::get(transforms)) else { return };
    let placed = attributes::create();
    attributes::set(placed, "g_TransformBuffer", attributes::Value::Buffer(transforms.value));
    let draws: Vec<Draw> = call_draws(c, mesh, vec![placed, attrs])
        .into_iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let mut d = d.filter(|d| d.indices.is_some() && d.shaded.is_some())?;
            let at = offset + i as u64 * INDEXED_ARGS;
            d.cull.indirect = indirect(args_at(args, at, INDEXED_ARGS)?, at);
            Some(d)
        })
        .collect();
    immediate::submit(c, &draws);
    objects::destroy(placed);
}

pub fn register(r: &mut Registry) {
    bind!(r, ctx::DRAW_INSTANCED_INDIRECT, |c, prim, args, offset, count, stride| draw_bound(c, prim, args, offset, count, stride, false));
    bind!(r, ctx::DRAW_INDEXED_INSTANCED_INDIRECT, |c, prim, args, offset, count, stride| draw_bound(c, prim, args, offset, count, stride, true));
    bind!(r, tools::DRAW_MODEL_1, |c, _layer, _mode, m, args, offset, attrs| instanced(c, m, 0, (args, offset), attrs));
    bind!(r, tools::DRAW_MODEL_3, |c, _layer, _mode, m, args, offset, attrs, lod| instanced(c, m, lod, (args, offset), attrs));
    bind!(r, tools::DRAW_MODEL_4, |c, _layer, _mode, m, transforms, args, offset, attrs, lod| transformed(c, m, lod, transforms, (args, offset), attrs));
}

#[cfg(test)]
#[path = "indirect_tests.rs"]
mod tests;
