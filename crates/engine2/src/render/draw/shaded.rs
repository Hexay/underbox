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
use render::{BufferKind, PipelineKey, VertexBufferKey};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

pub struct Shaded {
    pub shading: Arc<Shading>,
    /// Slot 0 from the mesh, slot 1 defaults, slot 2 instance ids ([`vertex::inputs`]).
    pub vertex: Vec<VertexBufferKey>,
    pub material: Arc<Material>,
    /// Values the draw adds over the material's (stand-in flags, normal packing).
    pub values: HashMap<String, Vec4>,
    pub instance: binding::Instance,
    pub view_proj: Mat4,
    pub context: ContextBindings,
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
    let (shading, extra) = shading::for_shader(gpu, material.shader())?;
    let Some(inputs) = vertex::inputs(&shading.vertex_inputs, layout) else {
        warn_once(&shading.label, &format!("mesh layout '{}' can't feed its vertex inputs", layout.name));
        return None;
    };
    let mut values: HashMap<String, Vec4> = extra.into_iter().map(|(k, v)| (k.to_owned(), Vec4::splat(v))).collect();
    values.insert("UncompressedTangentFrame".into(), Vec4::splat(f32::from(u8::from(inputs.uncompressed_normals))));
    Some(Shaded { shading, vertex: inputs.keys, material, values, instance, view_proj, context })
}

/// Slot 1 (attribute defaults, stride 0) and slot 2 (instance ids from 0) for every shaded draw.
pub fn streams(gpu: &render::Gpu) -> &'static (render::Buffer, render::Buffer) {
    static STREAMS: OnceLock<(render::Buffer, render::Buffer)> = OnceLock::new();
    STREAMS.get_or_init(|| {
        let ids: Vec<u8> = (0..MAX_INSTANCES).flat_map(u32::to_le_bytes).collect();
        let defaults: Vec<u8> = vertex::DEFAULTS.iter().flat_map(|f| f.to_le_bytes()).collect();
        (render::Buffer::with_data(gpu, "material defaults", BufferKind::Vertex, &defaults), render::Buffer::with_data(gpu, "instance ids", BufferKind::Vertex, &ids))
    })
}

/// Instance ids the shared id stream holds; more instances in one draw read past it.
pub const MAX_INSTANCES: u32 = 1024;

/// The draw's bind groups, with its pipeline for `key` built: both under one validation scope.
pub fn bind(gpu: &render::Gpu, d: &Shaded, view: &View, lighting: &Lighting, key: &PipelineKey) -> Option<Vec<wgpu::BindGroup>> {
    let (textures, vectors) = d.material.shading_params();
    let instances = [d.instance];
    let inputs = binding::Inputs { view, lighting, instances: &instances, textures: &textures, values: &[&d.values, &vectors], attributes: &d.context.attributes, slots: &d.context.slots };
    let built = gpu.checked(|| binding::bind_groups(gpu, &d.shading, &inputs)).and_then(|groups| groups);
    let built = built.and_then(|groups| d.shading.program.try_pipeline(gpu, key).map(|_| groups));
    built.inspect_err(|e| warn_once(&d.shading.label, e)).ok()
}

fn warn_once(label: &str, e: &str) {
    static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    if WARNED.lock().unwrap().insert(label.to_owned()) {
        let e: Vec<_> = e.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{label}: placeholder instead ({})", e.join(" ")));
    }
}
