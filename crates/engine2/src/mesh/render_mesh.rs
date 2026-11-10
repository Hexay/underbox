//! `CRenderMesh` (managed `Mesh`): material, primitive type, bounds, vertex/index buffers, draw
//! calls and morphs, kept on the CPU. Models built at runtime share these with managed, so later
//! buffer writes show through. The renderer reads [`RenderMesh::state`] and the buffers.

use super::buffers::{self, IndexBuffer, VertexBuffer};
use super::layout::Layout;
use super::material_ref::MaterialRef;
use super::vertex::Vertex;
use crate::math::from_v3;
use crate::resource::{self, Resource};
use abi::slots::mesh_glue as g;
use abi::structs::BBox;
use glam::Vec3;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex, MutexGuard};

pub type Ref = NativeRef<class::CRenderMesh>;

/// `RENDER_PRIM_TRIANGLES` (RenderPrimitiveType.cs).
pub const TRIANGLES: i32 = 5;

#[derive(Clone, Default)]
pub struct DrawCall {
    pub material: Option<Arc<MaterialRef>>,
    pub prim: i32,
    pub start_index: i32,
    pub index_count: i32,
    pub start_vertex: i32,
    pub vertex_count: i32,
}

/// One morph target: per-vertex position and normal offsets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Morph {
    pub name: String,
    pub deltas: Vec<MorphDelta>,
}

/// managed's `MorphDelta` (Mesh.Morphs.cs), 28 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MorphDelta {
    pub vertex: i32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// managed's `Mesh.MorphNativeDesc`: a name span of `morphNames` and a span of the deltas.
#[repr(C)]
#[derive(Clone, Copy)]
struct MorphDesc {
    name_offset: i32,
    name_length: i32,
    start: i32,
    count: i32,
}

#[derive(Default)]
pub struct MeshState {
    pub material: Option<Arc<MaterialRef>>,
    pub prim: i32,
    pub bounds: Option<BBox>,
    pub uv_density: f32,
    pub vertex_range: (i32, i32),
    pub index_range: (i32, i32),
    /// Explicit draw calls; without any the mesh draws once with its own material and ranges.
    pub draw_calls: Vec<DrawCall>,
    pub vertices: Option<Arc<VertexBuffer>>,
    pub indices: Option<Arc<IndexBuffer>>,
    pub morphs: Vec<Morph>,
}

pub struct RenderMesh {
    pub name: String,
    state: Mutex<MeshState>,
}

impl Resource for RenderMesh {}

impl Drop for RenderMesh {
    fn drop(&mut self) {
        let s = self.state.get_mut().unwrap();
        s.vertices.iter().for_each(|b| buffers::release_vertex(b.id()));
        s.indices.iter().for_each(|b| buffers::release_index(b.id()));
    }
}

impl RenderMesh {
    pub fn new(name: &str, material: Option<Arc<MaterialRef>>, prim: i32) -> Arc<Self> {
        Arc::new(Self { name: name.to_owned(), state: Mutex::new(MeshState { material, prim, ..Default::default() }) })
    }

    /// A triangle mesh over owned data (compiled geometry), with its material by name unresolved.
    pub fn from_vertices(name: &str, vertices: &[Vertex], indices: &[u32]) -> Arc<Self> {
        let mesh = Self::new(name, None, TRIANGLES);
        {
            let mut s = mesh.state();
            let bytes = Vertex::as_bytes(vertices).to_vec();
            s.vertices = Some(Arc::new(VertexBuffer::new(Layout::sbox_vertex(), bytes)));
            s.indices = Some(Arc::new(IndexBuffer::new(true, indices.iter().flat_map(|i| i.to_le_bytes()).collect())));
            s.vertex_range = (0, vertices.len() as i32);
            s.index_range = (0, indices.len() as i32);
        }
        mesh
    }

    pub fn state(&self) -> MutexGuard<'_, MeshState> {
        self.state.lock().unwrap()
    }

    /// The draw calls the mesh makes, explicit or implied.
    pub fn draw_calls(&self) -> Vec<DrawCall> {
        let s = self.state();
        if !s.draw_calls.is_empty() {
            return s.draw_calls.clone();
        }
        let vertex_count = if s.vertex_range.1 > 0 { s.vertex_range.1 } else { s.vertices.as_ref().map_or(0, |v| v.count() as i32) };
        let index_count = if s.index_range.1 > 0 { s.index_range.1 } else { s.indices.as_ref().map_or(0, |i| i.indices().len() as i32) };
        vec![DrawCall { material: s.material.clone(), prim: s.prim, start_index: s.index_range.0, index_count, start_vertex: s.vertex_range.0, vertex_count }]
    }

    /// Every vertex in the vertex buffer, decoded.
    pub fn vertices(&self) -> Vec<Vertex> {
        let vb = self.state().vertices.clone();
        vb.map(|b| b.meta.decode_all(&b.read())).unwrap_or_default()
    }

    pub fn indices(&self) -> Vec<u32> {
        let ib = self.state().indices.clone();
        ib.map(|b| b.indices()).unwrap_or_default()
    }

    pub fn streams(&self) -> i32 {
        self.state().vertices.as_ref().map_or(0, |b| b.meta.streams())
    }

    /// Bounds managed set, else those of the vertices.
    pub fn bounds(&self) -> BBox {
        if let Some(b) = self.state().bounds {
            return b;
        }
        bounds_of(self.vertices().iter().map(|v| Vec3::from(v.position)))
    }
}

/// The box around `points`; zero for none.
pub fn bounds_of(points: impl Iterator<Item = Vec3>) -> BBox {
    let (lo, hi, n) = points.fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0), |(lo, hi, n), p| (lo.min(p), hi.max(p), n + 1));
    if n == 0 { BBox::default() } else { BBox { mins: from_v3(lo), maxs: from_v3(hi) } }
}

pub fn get(r: Ref) -> Option<Arc<RenderMesh>> {
    resource::resolve::<_, RenderMesh>(r)
}

fn with(r: Ref, f: impl FnOnce(&mut MeshState)) {
    if let Some(m) = get(r) {
        f(&mut m.state());
    }
}

fn create(material: crate::render::material::Ref, prim: i32, name: Option<&CStr>) -> Ref {
    let name = name.map(CStr::to_string_lossy).unwrap_or_default();
    resource::new_handle(RenderMesh::new(&name, MaterialRef::keep(material), prim))
}

/// Bind a buffer, releasing the one it replaces.
fn bind_buffer<M>(slot: &mut Option<Arc<buffers::Buffer<M>>>, new: Option<Arc<buffers::Buffer<M>>>, release: fn(usize)) {
    if let (Some(old), Some(n)) = (slot.as_ref(), new.as_ref())
        && old.id() != n.id()
    {
        release(old.id());
    }
    if new.is_some() {
        *slot = new;
    }
}

fn set_morphs(r: Ref, descs: RawPtr<c_void>, n: i32, deltas: RawPtr<c_void>, n_deltas: i32, names: Option<&CStr>) {
    // SAFETY: managed passes pinned MorphNativeDesc[n] and MorphDelta[n_deltas] (ModelBuilder.Morphs.cs).
    let (descs, deltas) = unsafe { (descs.cast::<MorphDesc>().as_slice(n.max(0) as usize), deltas.cast::<MorphDelta>().as_slice(n_deltas.max(0) as usize)) };
    let names = names.map(CStr::to_bytes).unwrap_or_default();
    let span = |start: i32, len: i32| start.max(0) as usize..(start.max(0) + len.max(0)) as usize;
    let morphs = descs.iter().map(|d| Morph {
        name: String::from_utf8_lossy(names.get(span(d.name_offset, d.name_length)).unwrap_or_default()).into_owned(),
        deltas: deltas.get(span(d.start, d.count)).unwrap_or_default().to_vec(),
    });
    with(r, |s| s.morphs = morphs.collect());
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_render_mesh, RenderMesh);
    bind!(r, g::CREATE_RENDER_MESH, create);
    bind!(r, g::SET_MESH_MATERIAL, |m, mat| with(m, |s| s.material = MaterialRef::keep(mat)));
    bind!(r, g::SET_MESH_PRIM_TYPE, |m, prim| with(m, |s| s.prim = prim));
    bind!(r, g::SET_MESH_BOUNDS, |m, mins, maxs| with(m, |s| s.bounds = Some(BBox { mins, maxs })));
    bind!(r, g::SET_MESH_UV_DENSITY, |m, d| with(m, |s| s.uv_density = d));
    bind!(r, g::SET_MESH_VERTEX_RANGE, |m, start, count| with(m, |s| s.vertex_range = (start, count)));
    bind!(r, g::SET_MESH_INDEX_RANGE, |m, start, count| with(m, |s| s.index_range = (start, count)));
    bind!(r, g::ADD_MESH_DRAW_CALL, |m, mat, prim, start_index, index_count, start_vertex, vertex_count| {
        let call = DrawCall { material: MaterialRef::keep(mat), prim, start_index, index_count, start_vertex, vertex_count };
        let mut n = -1;
        with(m, |s| {
            s.draw_calls.push(call);
            n = s.draw_calls.len() as i32 - 1;
        });
        n
    });
    // The data was uploaded when the buffer was made; these only bind it (Mesh.VertexBuffer.cs).
    bind!(r, g::SET_MESH_VERTEX_BUFFER, |m, h, _, _| with(m, |s| bind_buffer(&mut s.vertices, buffers::vertex_buffer(h), buffers::release_vertex)));
    bind!(r, g::SET_MESH_INDEX_BUFFER, |m, h, _, _| with(m, |s| bind_buffer(&mut s.indices, buffers::index_buffer(h), buffers::release_index)));
    bind!(r, g::SET_MESH_MORPH_DATA, set_morphs);
}
