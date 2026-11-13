//! Draws shaded by their material's program (render/shading) instead of the placeholder. A
//! [`Shaded`] is resolved when the draw is built (material → shader or stand-in → program →
//! vertex inputs from the mesh layout); [`bind`] builds its bind groups and pipeline for the
//! pass under a validation scope. Any failure falls back to the placeholder, logged once per
//! shader.

use crate::mesh::layout::Layout;
use crate::render::material::Material;
use crate::render::shading::{self, Shading, bind as binding, vertex, view::View};
use crate::render::lighting::Lighting;
use crate::render::{attributes, device, texture};
use glam::{Mat4, Vec4};
use render::wgpu;
use render::{BufferKind, PipelineKey, RasterState, VertexBufferKey};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

pub struct Shaded {
    pub shading: Arc<Shading>,
    /// Slot 0 from the mesh, then the shared [`Streams`] ([`vertex::inputs`]).
    pub vertex: Vec<VertexBufferKey>,
    pub material: Arc<Material>,
    /// Values the draw adds over the material's (stand-in flags, normal packing).
    pub values: HashMap<String, Vec4>,
    pub instance: binding::Instance,
    pub view_proj: Mat4,
    pub context: ContextBindings,
    /// The mesh layout, kept for vertex-cache draws: their depth program reads fields this one doesn't.
    pub cached_mesh: Option<Arc<Layout>>,
}

/// What a context binds for its draws besides the material (`SetRenderState`, `BindTexture`).
#[derive(Clone, Default)]
pub struct ContextBindings {
    /// Render attributes, most specific first.
    pub attributes: Vec<attributes::Ref>,
    pub slots: Vec<(u32, texture::Ref)>,
    /// Managed's instance-id buffer and byte offset, in place of the shared 0.. id stream.
    pub instance_ids: Option<(Arc<render::Buffer>, u64)>,
}

/// `material` drawing a mesh of `layout`, or `None` (placeholder) when its program or the
/// mesh's vertex inputs don't work out.
pub fn resolve(material: Arc<Material>, layout: &Layout, instance: binding::Instance, view_proj: Mat4, context: ContextBindings) -> Option<Shaded> {
    let gpu = device::gpu()?;
    let (shading, extra) = shading::for_shader(gpu, material.shader(), &context.attributes)?;
    let Some(inputs) = vertex::inputs(&shading.vertex_inputs, layout) else {
        warn_once(&shading.label, &format!("mesh layout '{}' can't feed its vertex inputs", layout.name));
        return None;
    };
    let mut values: HashMap<String, Vec4> = extra.into_iter().map(|(k, v)| (k.to_owned(), Vec4::splat(v))).collect();
    values.insert("UncompressedTangentFrame".into(), Vec4::splat(f32::from(u8::from(inputs.uncompressed_normals))));
    let cached_mesh = vertex::streams(&inputs.keys).contains(&vertex::Stream::VertexIndex).then(|| Arc::new(layout.clone()));
    Some(Shaded { shading, vertex: inputs.keys, material, values, instance, view_proj, context, cached_mesh })
}

impl Shaded {
    /// `base` under the program's declared render states. The material stand-in culls back faces
    /// unless the material sets `F_RENDER_BACKFACES`, as the closed shaders it replaces do
    /// (vr_common_ps_code.fxc's `MATERIAL_CONTROLLED_BACKFACES`).
    pub fn raster_state(&self, base: RasterState) -> RasterState {
        let base = if self.shading.label == shading::standin::PATH {
            RasterState { cull: (!self.material.feature("F_RENDER_BACKFACES")).then_some(wgpu::Face::Back), ..base }
        } else {
            base
        };
        self.shading.render_state.apply(base)
    }
}

/// The shared streams behind a shaded draw's slots after the mesh's ([`vertex::Stream`]).
pub struct Streams {
    pub defaults: render::Buffer,
    /// Instance ids from 0.
    pub ids: render::Buffer,
    /// Vertex indices from 0, as floats.
    pub indices: render::Buffer,
}

impl Streams {
    /// The buffers bound after the mesh for `keys`, with `ids` as the instance-id stream.
    pub fn bound<'a>(&'a self, keys: &[VertexBufferKey], ids: (&'a wgpu::Buffer, u64)) -> Vec<(&'a wgpu::Buffer, u64)> {
        let buffer = |s| match s {
            vertex::Stream::InstanceIds => ids,
            vertex::Stream::VertexIndex => (&self.indices.buffer, 0),
            vertex::Stream::Defaults => (&self.defaults.buffer, 0),
        };
        vertex::streams(keys).into_iter().map(buffer).collect()
    }
}

pub fn streams(gpu: &render::Gpu) -> &'static Streams {
    static STREAMS: OnceLock<Streams> = OnceLock::new();
    STREAMS.get_or_init(|| {
        let ids: Vec<u8> = (0..MAX_INSTANCES).flat_map(u32::to_le_bytes).collect();
        let defaults: Vec<u8> = vertex::DEFAULTS.iter().flat_map(|f| f.to_le_bytes()).collect();
        let indices: Vec<u8> = (0..MAX_VERTEX_INDEX).flat_map(|i| (i as f32).to_le_bytes()).collect();
        let buffer = |label, data: &[u8]| render::Buffer::with_data(gpu, label, BufferKind::Vertex, data);
        Streams { defaults: buffer("material defaults", &defaults), ids: buffer("instance ids", &ids), indices: buffer("vertex indices", &indices) }
    })
}

/// Instance ids the shared id stream holds; more instances in one draw read past it.
pub const MAX_INSTANCES: u32 = 1024;
/// Vertex indices the shared index stream holds (real's holds as many); larger vertex-cache
/// meshes read past it.
pub const MAX_VERTEX_INDEX: u32 = 1 << 17;

/// The draw's bind groups, with its pipeline for `key` built: both under one validation scope.
pub fn bind(gpu: &render::Gpu, d: &Shaded, view: &View, lighting: &Lighting, key: &PipelineKey, targets: &[texture::Ref]) -> Option<Vec<wgpu::BindGroup>> {
    let (textures, vectors) = d.material.shading_params();
    let instances = [d.instance];
    let attributes: Vec<_> = d.context.attributes.iter().copied().chain(lighting.shadow_attributes).collect();
    let inputs = binding::Inputs { view, lighting, instances: &instances, textures: &textures, values: &[&d.values, &vectors], attributes: &attributes, slots: &d.context.slots, targets };
    let built = gpu.checked(|| binding::bind_groups(gpu, &d.shading, &inputs)).and_then(|groups| groups);
    let built = built.and_then(|groups| d.shading.program.try_pipeline(gpu, key).map(|_| groups));
    built.inspect_err(|e| warn_once(&d.shading.label, e)).ok()
}

pub(crate) fn warn_once(label: &str, e: &str) {
    static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    if WARNED.lock().unwrap().insert(label.to_owned()) {
        let e: Vec<_> = e.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{label}: placeholder instead ({})", e.join(" ")));
    }
}
