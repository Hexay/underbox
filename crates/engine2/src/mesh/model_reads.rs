//! Reading a model's geometry back (`MeshGlue.GetModel*`, `ReadDrawCall`; Model.VertexData.cs).
//! Model-wide reads concatenate the meshes: indices are rebased onto the concatenated vertices,
//! and model-wide draw-call indices run across the meshes in order.

use super::render_mesh::{DrawCall, RenderMesh};
use super::vertex::Vertex;
use crate::model::{self, Model, Ref};
use abi::slots::mesh_glue as g;
use abi::{RawPtr, Registry, bind};
use std::ffi::c_void;
use std::sync::Arc;

/// Every vertex, then every index rebased onto them.
fn concatenated(m: &Model) -> (Vec<Vertex>, Vec<u32>) {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for x in &m.content.meshes {
        let base = vertices.len() as u32;
        vertices.extend(x.mesh.vertices());
        indices.extend(x.mesh.indices().into_iter().map(|i| i + base));
    }
    (vertices, indices)
}

/// Each draw call with the index and vertex offsets of its mesh in the concatenation.
fn flat_draw_calls(m: &Model) -> Vec<(DrawCall, i32, i32, u8)> {
    let (mut index_base, mut vertex_base, mut out) = (0, 0, Vec::new());
    for x in &m.content.meshes {
        out.extend(x.mesh.draw_calls().into_iter().map(|d| (d, index_base, vertex_base, x.lod_mask)));
        index_base += x.mesh.indices().len() as i32;
        vertex_base += x.mesh.vertices().len() as i32;
    }
    out
}

fn copy_out<T: Copy>(src: &[T], dst: RawPtr<c_void>, n: u32) {
    let n = src.len().min(n as usize);
    // SAFETY: managed passes a pinned T[n] (Model.VertexData.cs).
    unsafe { dst.cast::<T>().as_mut_slice(n) }.copy_from_slice(&src[..n]);
}

fn draw_call(r: Ref, mesh: i32, call: i32) -> Option<(Arc<RenderMesh>, DrawCall)> {
    model::on(r, |m| {
        let x = m.content.mesh(mesh)?;
        let d = x.mesh.draw_calls().into_iter().nth(usize::try_from(call).ok()?)?;
        Some((x.mesh.clone(), d))
    })
}

/// A draw call's vertices compacted to those its indices use, and its indices rebased onto them.
pub fn read_draw_call(mesh: &RenderMesh, d: &DrawCall) -> (Vec<Vertex>, Vec<u32>) {
    let (all, indices) = (mesh.vertices(), mesh.indices());
    let start = d.start_index.max(0) as usize;
    let span = indices.get(start..start + d.index_count.max(0) as usize).unwrap_or_default();
    let mut remap = std::collections::HashMap::new();
    let (mut vertices, mut out) = (Vec::new(), Vec::with_capacity(span.len()));
    for &i in span {
        let source = i as usize + d.start_vertex.max(0) as usize;
        let next = vertices.len() as u32;
        let new = *remap.entry(source).or_insert_with(|| {
            vertices.push(all.get(source).copied().unwrap_or_default());
            next
        });
        out.push(new);
    }
    (vertices, out)
}

#[allow(clippy::too_many_arguments)]
fn read(r: Ref, mesh: i32, call: i32, vertices: RawPtr<c_void>, nv: u32, indices: RawPtr<c_void>, ni: u32, used: &mut u32) -> bool {
    let Some((m, d)) = draw_call(r, mesh, call) else { return false };
    let (v, i) = read_draw_call(&m, &d);
    if v.len() > nv as usize || i.len() > ni as usize {
        return false;
    }
    copy_out(&v, vertices, nv);
    copy_out(&i, indices, ni);
    *used = v.len() as u32;
    true
}

fn on_flat<R: Default>(r: Ref, call: i32, f: impl FnOnce(&(DrawCall, i32, i32, u8)) -> R) -> R {
    model::on(r, |m| usize::try_from(call).ok().and_then(|c| flat_draw_calls(m).get(c).map(f)).unwrap_or_default())
}

pub fn register(r: &mut Registry) {
    bind!(r, g::GET_MODEL_NUM_VERTICES, |r| model::on(r, |m| m.content.meshes.iter().map(|x| x.mesh.vertices().len()).sum::<usize>() as i32));
    bind!(r, g::GET_MODEL_VERTICES, |r, out, n| copy_out(&model::on(r, concatenated).0, out, n));
    bind!(r, g::GET_MODEL_NUM_INDICES, |r| model::on(r, |m| m.content.meshes.iter().map(|x| x.mesh.indices().len()).sum::<usize>() as i32));
    bind!(r, g::GET_MODEL_INDICES, |r, out, n| copy_out(&model::on(r, concatenated).1, out, n));
    bind!(r, g::GET_DRAW_CALL_VERTEX_COUNT, |r, mesh, call| draw_call(r, mesh, call).map_or(0, |(_, d)| d.vertex_count));
    bind!(r, g::GET_DRAW_CALL_INDEX_COUNT, |r, mesh, call| draw_call(r, mesh, call).map_or(0, |(_, d)| d.index_count));
    bind!(r, g::GET_DRAW_CALL_STREAMS, |r, mesh, call| draw_call(r, mesh, call).map_or(0, |(m, _)| m.streams()));
    bind!(r, g::READ_DRAW_CALL, read);
    bind!(r, g::GET_MODEL_INDEX_COUNT, |r, call| on_flat(r, call, |d| d.0.index_count));
    bind!(r, g::GET_MODEL_INDEX_START, |r, call| on_flat(r, call, |d| d.1 + d.0.start_index));
    bind!(r, g::GET_MODEL_BASE_VERTEX, |r, call| on_flat(r, call, |d| d.2 + d.0.start_vertex));
    bind!(r, g::GET_MODEL_LOD_INDEX_COUNT, |r, lod: i32| {
        let bit = 1u8.checked_shl(lod.max(0) as u32).unwrap_or(0);
        model::on(r, |m| flat_draw_calls(m).iter().filter(|d| d.3 & bit != 0).map(|d| d.0.index_count).sum())
    });
}
