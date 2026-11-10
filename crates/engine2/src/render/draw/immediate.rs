//! Draws managed issues directly on a context: `RenderTools.SetRenderState` + `Draw` (dynamic
//! vertices), bound vertex/index buffers + `IRenderContext.Draw*`, and `DrawModel` /
//! `DrawSceneObject`. Each executes at once into the context's bound targets (context.rs).
//!
//! Positions go through the context's `view_proj`: the view's camera while a layer callback runs
//! (scene/execute.rs), identity otherwise. After `SetRenderState`, `Draw` and `Draw*` shade with
//! that mode's material program and attributes; the placeholder when it doesn't build or bind.

use super::{Draw, gpu_mesh, model as model_draw, topology};
use crate::mesh::buffers::{self, IndexBuffer, VertexBuffer};
use crate::mesh::layout::{self, Layout};
use super::shaded::{self, ContextBindings, Shaded};
use crate::render::lighting::Lighting;
use crate::render::shading::bind::Instance;
use crate::render::{attributes, buffers as gpu_buffers, context, device, material_mode, swap_chain, texture};
use abi::slots::{i_render_context as ctx, render_tools as tools};
use abi::structs::{Transform, Vector4};
use abi::{RawPtr, Registry, bind};
use glam::Mat4;
use render::pass::{DrawUniforms, Load};
use render::wgpu::IndexFormat;
use render::{BufferKind, RasterState};
use std::ffi::c_void;
use std::sync::Arc;

enum Vertices {
    Mesh(Arc<VertexBuffer>),
    Gpu(Arc<render::Buffer>),
}

enum Indices {
    Mesh(Arc<IndexBuffer>),
    Gpu(Arc<render::Buffer>, IndexFormat),
}

/// What `IRenderContext.Draw*` draw with: the layout from `SetRenderState`, bound buffers, and
/// the view transform.
pub struct DrawState {
    layout: Option<Arc<Layout>>,
    vertices: Option<(Vertices, u64)>,
    indices: Option<Indices>,
    pub view_proj: Mat4,
    /// The view's lights while a layer runs, [`Lighting::fallback`] otherwise.
    pub lighting: Arc<Lighting>,
    /// `SetRenderState`'s attributes and material mode, which later draws shade with.
    render_state: Option<(attributes::Ref, material_mode::Ref)>,
    /// `BindTexture` textures by slot.
    pub slots: Vec<(u32, texture::Ref)>,
}

impl Default for DrawState {
    fn default() -> Self {
        let lighting = Arc::new(Lighting::fallback());
        DrawState { layout: None, vertices: None, indices: None, view_proj: Mat4::IDENTITY, lighting, render_state: None, slots: Vec::new() }
    }
}

type Ctx = context::Ref;

pub(crate) fn with<R: Default>(c: Ctx, f: impl FnOnce(&mut DrawState) -> R) -> R {
    context::get(c).map(|c| f(&mut c.draw.lock().unwrap())).unwrap_or_default()
}

pub fn view_proj(c: Ctx) -> Mat4 {
    context::get(c).map_or(Mat4::IDENTITY, |c| c.draw.lock().unwrap().view_proj)
}

/// Draws on `c` go through `view_proj` and are lit by `lighting` from now on.
pub fn set_view(c: Ctx, view_proj: Mat4, lighting: Arc<Lighting>) {
    with(c, |s| (s.view_proj, s.lighting) = (view_proj, lighting));
}

pub fn lighting(c: Ctx) -> Arc<Lighting> {
    context::get(c).map_or_else(|| Arc::new(Lighting::fallback()), |c| c.draw.lock().unwrap().lighting.clone())
}

fn uniforms(c: Ctx, world: Mat4, tint: [f32; 4]) -> DrawUniforms {
    DrawUniforms { mvp: view_proj(c) * world, model: world, tint }
}

pub fn submit(c: Ctx, draws: &[Draw]) {
    if !draws.is_empty() {
        super::execute(&context::target(c), &lighting(c), Load::default(), draws);
    }
}

fn set_render_state(c: Ctx, attrs: attributes::Ref, mode: material_mode::Ref, l: layout::Ref) -> bool {
    with(c, |s| (s.layout, s.render_state) = (layout::get(l), Some((attrs, mode))));
    device::gpu().is_some()
}

/// The `SetRenderState` material's program for a draw of `layout` on `c`, bound with its
/// attributes, the context's and the `BindTexture` slots; `None` draws the placeholder.
fn render_state_shading(c: Ctx, layout: &Layout) -> Option<Shaded> {
    let ctx = context::get(c)?;
    let (state, slots, view_proj) = {
        let s = ctx.draw.lock().unwrap();
        (s.render_state, s.slots.clone(), s.view_proj)
    };
    let (attrs, mode) = state?;
    let bindings = ContextBindings { attributes: vec![attrs, ctx.attributes], slots, instance_ids: None };
    shaded::resolve(material_mode::material(mode)?, layout, Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }, view_proj, bindings)
}

/// `RenderTools.Draw`: managed's vertex (and optional u16 index) arrays, uploaded per call.
#[allow(clippy::too_many_arguments)]
fn draw_dynamic(c: Ctx, prim: i64, l: layout::Ref, vertices: RawPtr<c_void>, n: i32, indices: RawPtr<c_void>, n_indices: i32) {
    let (Some(gpu), Some(layout)) = (device::gpu(), layout::get(l).or_else(|| with(c, |s| s.layout.clone()))) else { return };
    let Some(vertex) = gpu_mesh::vertex_keys(&layout) else { return };
    if vertices.is_null() || n <= 0 {
        return;
    }
    // SAFETY: managed passes pinned arrays of n vertices of the layout's stride and n_indices u16s (Graphics.Draw.cs).
    let bytes = unsafe { vertices.cast::<u8>().as_slice(n as usize * layout.stride) };
    let vb = Arc::new(render::Buffer::with_data(gpu, "dynamic vertices", BufferKind::Vertex, bytes));
    let ib = (!indices.is_null() && n_indices > 0).then(|| {
        let idx = unsafe { indices.cast::<u8>().as_slice(n_indices as usize * 2) };
        (Arc::new(render::Buffer::with_data(gpu, "dynamic indices", BufferKind::Index, idx)), IndexFormat::Uint16)
    });
    let range = 0..if ib.is_some() { n_indices as u32 } else { n as u32 };
    let state = RasterState { topology: topology(prim), ..Default::default() };
    let shaded = render_state_shading(c, &layout);
    let draw = Draw { vertices: vb, vertex_offset: 0, vertex, indices: ib, range, base_vertex: 0, instances: 1, uniforms: uniforms(c, Mat4::IDENTITY, [1.0; 4]), state, shaded };
    submit(c, &[draw]);
}

/// A draw from the bound buffers: `range` is indices when an index buffer is bound and `indexed`.
fn draw_bound(c: Ctx, prim: i64, range: std::ops::Range<i32>, instances: i32, base_vertex: i32, indexed: bool) {
    let Some((mut draw, layout)) = with(c, |s| bound_draw(s, prim, range, instances, base_vertex, indexed)) else { return };
    draw.shaded = render_state_shading(c, &layout);
    submit(c, &[draw]);
}

fn bound_draw(s: &DrawState, prim: i64, range: std::ops::Range<i32>, instances: i32, base_vertex: i32, indexed: bool) -> Option<(Draw, Arc<Layout>)> {
    let (src, offset) = s.vertices.as_ref()?;
    let (vertices, layout) = match src {
        Vertices::Mesh(b) => (gpu_mesh::vertices(b)?, s.layout.clone().unwrap_or_else(|| b.meta.clone())),
        Vertices::Gpu(b) => (b.clone(), s.layout.clone()?),
    };
    let indices = match (&s.indices, indexed) {
        (Some(Indices::Mesh(b)), true) => Some(gpu_mesh::indices(b)?),
        (Some(Indices::Gpu(b, f)), true) => Some((b.clone(), *f)),
        _ => None,
    };
    let range = range.start.max(0) as u32..range.end.max(0) as u32;
    let uniforms = DrawUniforms { mvp: s.view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let state = RasterState { topology: topology(prim), ..Default::default() };
    let vertex = gpu_mesh::vertex_keys(&layout)?;
    Some((Draw { vertices, vertex_offset: *offset, vertex, indices, range, base_vertex, instances: instances.max(1) as u32, uniforms, state, shaded: None }, layout))
}

fn bind_vertices(c: Ctx, slot: i32, v: Option<Vertices>, offset: i32) -> bool {
    let ok = v.is_some();
    // Only stream 0 feeds the placeholder; instance streams are ignored until real shaders.
    if slot == 0 {
        with(c, |s| s.vertices = v.map(|v| (v, offset.max(0) as u64)));
    }
    ok
}

fn bind_indices(c: Ctx, i: Option<Indices>) -> bool {
    let ok = i.is_some();
    with(c, |s| s.indices = i);
    ok
}

/// `count` transforms managed passes (`Transform[]`), or one identity for a null pointer.
fn transforms(p: RawPtr<c_void>, count: i32) -> Vec<Mat4> {
    if p.is_null() {
        return vec![Mat4::IDENTITY; count.clamp(0, 1) as usize];
    }
    // SAFETY: managed passes a pinned Span<Transform> of `count` (Graphics.Draw.Mesh.cs).
    unsafe { p.cast::<Transform>().as_slice(count.max(0) as usize) }.iter().map(model_draw::matrix).collect()
}

fn draw_model(c: Ctx, m: crate::model::Ref, xforms: Vec<Mat4>) {
    let Some(model) = crate::model::get(m) else { return };
    let vp = view_proj(c);
    let draws: Vec<_> = xforms.into_iter().flat_map(|w| model_draw::draws(&model, w, vp, [1.0; 4], u64::MAX, None)).collect();
    submit(c, &draws);
}

pub(crate) fn draw_scene_object(c: Ctx, o: crate::scene::object::Ref, t: Transform, color: Vector4) {
    let Some(obj) = crate::scene::object::get(o) else { return };
    let (m, groups, tint, material) = {
        let s = obj.state.lock().unwrap();
        (s.model, s.mesh_groups, model_draw::tint(s.tint), s.material_override.clone())
    };
    let Some(model) = crate::model::get(m) else { return };
    let color = model_draw::tint(color);
    let tint = std::array::from_fn(|i| tint[i] * color[i]);
    submit(c, &model_draw::draws(&model, model_draw::matrix(&t), view_proj(c), tint, groups, material.as_ref()));
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::SET_RENDER_STATE, |c, a, m, l, _stats| set_render_state(c, a, m, l));
    bind!(r, tools::DRAW, |c, prim, l, v, n, i, ni, _stats| draw_dynamic(c, prim, l, v, n, i, ni));
    bind!(r, ctx::DRAW, |c, prim, first, count| draw_bound(c, prim, first..first + count, 1, 0, false));
    bind!(r, ctx::DRAW_INSTANCED, |c, prim, first, count, n| draw_bound(c, prim, first..first + count, n, 0, false));
    bind!(r, ctx::DRAW_INDEXED, |c, prim, first, count, _max, base| draw_bound(c, prim, first..first + count, 1, base, true));
    bind!(r, ctx::DRAW_INDEXED_INSTANCED, |c, prim, first, count, n, _max, base| draw_bound(c, prim, first..first + count, n, base, true));
    // TODO: indirect draws need the args buffer read on the GPU (draw_indirect).
    bind!(r, ctx::DRAW_INSTANCED_INDIRECT, |_, _, _, _, _, _| {});
    bind!(r, ctx::DRAW_INDEXED_INSTANCED_INDIRECT, |_, _, _, _, _, _| {});
    bind!(r, ctx::BIND_VERTEX_BUFFER, |c, slot, h, off| bind_vertices(c, slot, buffers::vertex_buffer(h).map(Vertices::Mesh), off));
    bind!(r, ctx::BIND_VERTEX_BUFFER_1, |c, slot, h, off, _stride| bind_vertices(c, slot, buffers::vertex_buffer(h).map(Vertices::Mesh), off));
    bind!(r, ctx::BIND_VERTEX_BUFFER_2, |c, slot, h, off| bind_vertices(c, slot, gpu_buffers::get(h).map(Vertices::Gpu), off));
    bind!(r, ctx::BIND_VERTEX_BUFFER_3, |c, slot, h, off, _stride| bind_vertices(c, slot, gpu_buffers::get(h).map(Vertices::Gpu), off));
    bind!(r, ctx::BIND_INDEX_BUFFER, |c, h, _off| bind_indices(c, buffers::index_buffer(h).map(Indices::Mesh)));
    bind!(r, ctx::BIND_INDEX_BUFFER_1, |c, h, size, _off| {
        let format = if size == 4 { IndexFormat::Uint32 } else { IndexFormat::Uint16 };
        bind_indices(c, gpu_buffers::get(h).map(|b| Indices::Gpu(b, format)))
    });
    bind!(r, ctx::BIND_RENDER_TARGETS_2, |c, chain, color, depth| {
        let (sc, sd) = swap_chain::targets(chain).unwrap_or_default();
        context::bind_targets(c, if color { sc } else { Default::default() }, if depth { sd } else { Default::default() });
    });
    bind!(r, tools::DRAW_MODEL, |c, _layer, _mode, m, xf, n, _attrs| draw_model(c, m, transforms(xf, n)));
    bind!(r, tools::DRAW_MODEL_2, |c, _layer, _mode, m, xf, n, _attrs, _lod| draw_model(c, m, transforms(xf, n)));
    // TODO: GPU-buffer transforms / draw args (DrawModel_1/3/4) need reading back or indirect draws.
    bind!(r, tools::DRAW_MODEL_1, |c, _layer, _mode, m, _buf, n, _attrs| draw_model(c, m, vec![Mat4::IDENTITY; n.clamp(0, 1) as usize]));
    bind!(r, tools::DRAW_MODEL_3, |c, _layer, _mode, m, _buf, n, _attrs, _lod| draw_model(c, m, vec![Mat4::IDENTITY; n.clamp(0, 1) as usize]));
    bind!(r, tools::DRAW_MODEL_4, |_, _, _, _, _, _, _, _, _| {});
    bind!(r, tools::DRAW_SCENE_OBJECT, |c, _layer, o, t, color, _mat, _attrs| draw_scene_object(c, o, t, color));
}
