//! Morph composites as native's (docs/skinning.md, "Morphs"): each frame's morphed meshes get a
//! region of one 2048×1024 `Rgba16Float` texture, summed by additive quads, one per morph target
//! (`render::Morpher`): position deltas in the left half, normal deltas at the same place in the
//! right half. A region `w` texels wide holds vertex `y * w + x` at texel (x, y) from its corner.
//! Its subrect, which managed stores in the morph entry and our `morph.fxc` reads, is
//! (x, width, y, height) of the position block in texels, as real's `GetMorphSubrect` answers.

use crate::mesh::render_mesh::RenderMesh;
use crate::render::draw::morph::{self, Weights};
use crate::render::{device, texture};
use abi::slots::render_tools as tools;
use abi::structs::Vector4;
use abi::{Registry, bind};
use render::{MorphBatch, MorphQuad};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

pub const WIDTH: u32 = 2048;
pub const HEIGHT: u32 = 1024;
const HALF: u32 = WIDTH / 2;
/// managed's `ImageFormat.RGBA16161616F`.
const RGBA16F: i32 = 24;

/// A mesh's targets on the GPU: target `t`'s position deltas at float4 `2tn`, normals at `2tn + n`.
struct Source {
    mesh: Weak<RenderMesh>,
    shape: Vec<(String, usize)>,
    vertices: u32,
    buffer: Arc<render::Buffer>,
}

fn source(mesh: &Arc<RenderMesh>) -> Option<(Arc<render::Buffer>, Vec<String>, u32)> {
    static SOURCES: LazyLock<Mutex<HashMap<usize, Source>>> = LazyLock::new(Default::default);
    let s = mesh.state();
    let n = s.vertices.as_ref().map_or(0, |v| v.count());
    let shape: Vec<(String, usize)> = s.morphs.iter().map(|m| (m.name.clone(), m.deltas.len())).collect();
    let mut sources = SOURCES.lock().unwrap();
    sources.retain(|_, src| src.mesh.strong_count() > 0);
    let key = Arc::as_ptr(mesh) as usize;
    if let Some(src) = sources.get(&key).filter(|src| src.shape == shape && src.vertices as usize == n && src.mesh.upgrade().is_some_and(|m| Arc::ptr_eq(&m, mesh))) {
        return Some((src.buffer.clone(), shape.into_iter().map(|t| t.0).collect(), src.vertices));
    }
    let mut data = vec![0f32; s.morphs.len() * 2 * n * 4];
    for (t, m) in s.morphs.iter().enumerate() {
        for d in m.deltas.iter().filter(|d| d.vertex >= 0 && (d.vertex as usize) < n) {
            let at = (2 * t * n + d.vertex as usize) * 4;
            data[at..at + 3].copy_from_slice(&d.position);
            data[at + n * 4..at + n * 4 + 3].copy_from_slice(&d.normal);
        }
    }
    data.resize(data.len().max(4), 0.0);
    let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
    let buffer = Arc::new(render::Buffer::with_data(device::gpu()?, "morph targets", render::BufferKind::Storage, &bytes));
    sources.insert(key, Source { mesh: Arc::downgrade(mesh), shape: shape.clone(), vertices: n as u32, buffer: buffer.clone() });
    Some((buffer, shape.into_iter().map(|t| t.0).collect(), n as u32))
}

/// This frame's composite so far: whether it was cleared, and where the next region goes
/// (shelves from (1, 1), a texel apart, as real's).
struct Frame {
    used: bool,
    x: u32,
    y: u32,
    shelf: u32,
}

const START: Frame = Frame { used: false, x: 1, y: 1, shelf: 0 };

impl Frame {
    fn place(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        if self.x + w + 1 > HALF {
            (self.x, self.y, self.shelf) = (1, self.y + self.shelf + 1, 0);
        }
        if self.y + h + 1 > HEIGHT {
            return None;
        }
        let at = [self.x, self.y];
        (self.x, self.shelf) = (self.x + w + 1, self.shelf.max(h));
        Some(at)
    }
}

static FRAME: Mutex<Frame> = Mutex::new(START);

/// The composite texture, made on first use.
pub fn texture() -> texture::Ref {
    static TEXTURE: OnceLock<usize> = OnceLock::new();
    let addr = *TEXTURE.get_or_init(|| texture::create_target("morph composite", WIDTH, HEIGHT, RGBA16F).as_raw() as usize);
    texture::Ref::from_raw(addr as *mut std::ffi::c_void)
}

fn morpher(gpu: &render::Gpu) -> &'static render::Morpher {
    static MORPHER: OnceLock<render::Morpher> = OnceLock::new();
    MORPHER.get_or_init(|| render::Morpher::new(gpu))
}

/// Whether any of `mesh`'s targets has weight.
pub fn active(mesh: &RenderMesh, weights: &Weights) -> bool {
    mesh.state().morphs.iter().any(|m| weights.get(&m.name).is_some_and(|w| *w != 0.0))
}

/// Composite `items` into this frame's texture in one pass (the frame's first clears it): each
/// item's subrect, zero when it didn't fit.
pub fn compose(items: &[(&Weights, &Arc<RenderMesh>)]) -> Vec<[f32; 4]> {
    let none = || vec![[0.0; 4]; items.len()];
    let Some(gpu) = device::gpu() else { return none() };
    let Some(target) = texture::resolve(texture()) else { return none() };
    let Some(target) = target.gpu.as_ref() else { return none() };
    let mut frame = FRAME.lock().unwrap();
    let clear = !frame.used;
    frame.used = true;
    let (mut subrects, mut sources, mut placed) = (Vec::new(), Vec::new(), Vec::new());
    for (weights, mesh) in items {
        let region = source(mesh).and_then(|(buffer, names, n)| {
            let w = n.clamp(1, HALF - 2);
            let h = n.div_ceil(w).max(1);
            frame.place(w, h).map(|[x, y]| (buffer, names, n, [x, y, w, h]))
        });
        let Some((buffer, names, n, rect)) = region else {
            subrects.push([0.0; 4]);
            continue;
        };
        let [x, y, w, h] = rect;
        subrects.push([x, w, y, h].map(|t| t as f32));
        let weight = |name: &String| weights.get(name).copied().unwrap_or(0.0);
        let quads = |bundle: u32, dx: u32| names.iter().enumerate().map(|(t, name)| MorphQuad { dst: [x + dx, y, w, h], first: (2 * t as u32 + bundle) * n, vertices: n, weight: weight(name) }).collect::<Vec<_>>();
        placed.push((quads(0, 0), quads(1, HALF)));
        sources.push(buffer);
    }
    let (positions, normals): (Vec<_>, Vec<_>) = placed.into_iter().unzip();
    let batches: Vec<MorphBatch> = sources.iter().zip(positions).map(|(s, quads)| MorphBatch { source: s, quads }).chain(sources.iter().zip(normals).map(|(s, quads)| MorphBatch { source: s, quads })).collect();
    morpher(gpu).composite(gpu, target, clear, &batches);
    subrects
}

/// A new frame: the next composite clears the texture and places regions from the corner.
pub fn begin_frame() {
    *FRAME.lock().unwrap() = START;
}

struct Queued {
    weights: Weights,
    mesh: Arc<RenderMesh>,
}

#[derive(Default)]
struct State {
    queued: Vec<Queued>,
    /// The last [`generate`]'s subrects, by queue handle.
    subrects: Vec<[f32; 4]>,
}

static STATE: Mutex<State> = Mutex::new(State { queued: Vec::new(), subrects: Vec::new() });

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

/// `GenerateMorphs`: composite everything queued since the last call, in one pass.
fn generate() {
    let queued = std::mem::take(&mut STATE.lock().unwrap().queued);
    let items: Vec<_> = queued.iter().map(|q| (&q.weights, &q.mesh)).collect();
    let subrects = compose(&items);
    STATE.lock().unwrap().subrects = subrects;
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
