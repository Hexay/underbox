//! Morph composites for skinning_cs: each queued (object, mesh) gets two `float4`s per vertex,
//! the summed position delta (w: wrinkle, 0) and normal delta of its weighted targets, packed one
//! after another in one buffer. A subrect is `(first float4, float4 count, 0, 0)`, read back by
//! our `morph.fxc`. Native composites on the GPU (`morph_composite`, no source); the sums are
//! the same.

use crate::mesh::render_mesh::RenderMesh;
use crate::render::draw::morph::{self, Weights};
use crate::render::device;
use abi::slots::render_tools as tools;
use abi::structs::Vector4;
use abi::{Registry, bind};
use std::sync::{Arc, Mutex};

struct Queued {
    weights: Weights,
    mesh: Arc<RenderMesh>,
}

#[derive(Default)]
struct State {
    queued: Vec<Queued>,
    /// The last [`generate`]'s buffer and subrects, by queue handle.
    buffer: Option<Arc<render::Buffer>>,
    subrects: Vec<[f32; 4]>,
}

static STATE: Mutex<State> = Mutex::new(State { queued: Vec::new(), buffer: None, subrects: Vec::new() });

/// Whether any of `mesh`'s targets has weight.
pub fn active(mesh: &RenderMesh, weights: &Weights) -> bool {
    mesh.state().morphs.iter().any(|m| weights.get(&m.name).is_some_and(|w| *w != 0.0))
}

/// The composite of `items` as float4 words, and each item's subrect.
pub fn compose(items: &[(&Weights, &RenderMesh)]) -> (Vec<f32>, Vec<[f32; 4]>) {
    let (mut data, mut subrects) = (Vec::new(), Vec::new());
    for (weights, mesh) in items {
        let first = data.len() / 4;
        let s = mesh.state();
        let n = s.vertices.as_ref().map_or(0, |v| v.count());
        data.resize(data.len() + n * 8, 0.0);
        for m in &s.morphs {
            let Some(w) = weights.get(&m.name).copied().filter(|w| *w != 0.0) else { continue };
            for d in m.deltas.iter().filter(|d| (d.vertex as usize) < n && d.vertex >= 0) {
                let at = (first + 2 * d.vertex as usize) * 4;
                (0..3).for_each(|i| data[at + i] += d.position[i] * w);
                (0..3).for_each(|i| data[at + 4 + i] += d.normal[i] * w);
            }
        }
        subrects.push([first as f32, (2 * n) as f32, 0.0, 0.0]);
    }
    (data, subrects)
}

/// `data` as a storage buffer (at least one float4).
pub fn upload(data: &[f32]) -> Option<Arc<render::Buffer>> {
    let gpu = device::gpu()?;
    let mut bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
    bytes.resize(bytes.len().max(16), 0);
    Some(Arc::new(render::Buffer::with_data(gpu, "morph composite", render::BufferKind::Storage, &bytes)))
}

/// `QueueModelMeshMorph`: a handle for [`subrect`] after [`generate`], or -1 when no target of
/// the mesh has weight.
fn queue(o: crate::scene::object::Ref, m: crate::model::Ref, mesh: i32) -> i32 {
    let (Some(weights), Some(model)) = (morph::of(o), crate::model::get(m)) else { return -1 };
    let Some((i, entry)) = usize::try_from(mesh).ok().and_then(|i| Some((i, model.content.meshes.get(i)?))) else { return -1 };
    let Some(w) = weights.mesh(i).filter(|w| active(&entry.mesh, w)) else { return -1 };
    let mut s = STATE.lock().unwrap();
    s.queued.push(Queued { weights: w.clone(), mesh: entry.mesh.clone() });
    s.queued.len() as i32 - 1
}

/// `GenerateMorphs`: composite everything queued since the last call.
fn generate() {
    let queued = std::mem::take(&mut STATE.lock().unwrap().queued);
    let items: Vec<_> = queued.iter().map(|q| (&q.weights, &*q.mesh)).collect();
    let (data, subrects) = compose(&items);
    let buffer = upload(&data);
    let mut s = STATE.lock().unwrap();
    (s.buffer, s.subrects) = (buffer, subrects);
}

/// The last composite (what a morphed `DispatchModelMeshSkinning` reads).
pub fn current() -> Option<Arc<render::Buffer>> {
    STATE.lock().unwrap().buffer.clone()
}

fn subrect(h: i32) -> [f32; 4] {
    let s = STATE.lock().unwrap();
    usize::try_from(h).ok().and_then(|i| s.subrects.get(i).copied()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::QUEUE_MODEL_MESH_MORPH, queue);
    bind!(r, tools::GENERATE_MORPHS, |_| generate());
    bind!(r, tools::GET_MORPH_SUBRECT, |h, out| {
        let [x, y, z, w] = subrect(h);
        // SAFETY: managed passes a Vector4 on its stack (RenderContext.MorphSubrect).
        unsafe { *out.cast::<Vector4>().as_ptr() = crate::math::vec4(x, y, z, w) };
    });
}
