//! One `skinning_cs` dispatch: a mesh's instances, each from its transform-buffer slot into its
//! vertex-cache block, optionally morphed from a composite ([`super::morphs`]). The shader's
//! inputs are attributes on a scratch `CRenderAttributes`, as native's dispatch sets them.

use super::{MeshSkin, NONE, cache};
use crate::render::attributes::{self, Value};
use crate::render::{buffers, compute, device, material_mode};
use abi::slots::render_tools as tools;
use abi::{RawPtr, Registry, StringToken, bind};
use std::ffi::c_void;
use std::sync::Arc;

pub const SHADER: &str = "shaders/skinning_cs.shader";
/// `Instances_t`'s `g_instances[256]`.
const MAX_INSTANCES: usize = 256;

/// One instance: its base entry in the transform buffer and first vertex in the cache.
#[derive(Clone, Copy, Debug)]
pub struct Job {
    pub slot: u32,
    pub offset: u32,
}

/// A buffer handle that lives for one dispatch.
struct Scratch(buffers::Handle);

impl Drop for Scratch {
    fn drop(&mut self) {
        buffers::destroy(self.0);
    }
}

fn instances(jobs: &[Job], weights: u32) -> Vec<u8> {
    let mut out = vec![0u8; MAX_INSTANCES * 32];
    for (j, chunk) in jobs.iter().zip(out.as_chunks_mut::<32>().0) {
        let words = [0, j.offset, (j.slot << 4) | weights.min(15), 0, 0, 0, 0, 0];
        chunk.copy_from_slice(&words.map(u32::to_le_bytes).concat());
    }
    out
}

/// Skin `jobs` of `skin`'s mesh with `transforms`, morphed from this frame's composite when `morph`.
pub fn run(shader: &str, skin: &MeshSkin, transforms: &Arc<render::Buffer>, jobs: &[Job], morph: bool) -> Result<(), String> {
    let gpu = device::gpu().ok_or("no device")?;
    let input = crate::render::draw::gpu_mesh::draw_vertices(&skin.vb).ok_or("no vertex buffer")?.0;
    let output = cache::buffer().ok_or("no vertex cache")?;
    let mut scratch = Vec::new();
    let mut handle = |b: Arc<render::Buffer>| {
        let h = buffers::insert(b);
        scratch.push(Scratch(h));
        h.value
    };
    let a = attributes::create();
    attributes::set(a, "g_TransformBuffer", Value::Buffer(handle(transforms.clone())));
    attributes::set(a, "InputVB", Value::Buffer(handle(input)));
    attributes::set(a, "OutputVB", Value::Buffer(handle(output)));
    if morph {
        attributes::set(a, "MorphComposite", Value::Texture(super::morphs::texture(), 0));
    }
    let i = skin.input;
    let atlas = if morph { super::morphs::WIDTH } else { 0 };
    // Gotcha: uint members fill from floats (shading/layout.rs); u32::MAX as f32 saturates back to NONE.
    for (name, v) in [("VertexCount", skin.vertices), ("VertexSize", i.stride), ("BoneIdxOffset", i.bone_index), ("BoneIdxBits", 8), ("BoneWeightOffset", i.bone_weight), ("NormalOffset", i.normal), ("TangentSpaceOffset", i.tangent), ("MorphTextureAtlasWidth", atlas)] {
        attributes::set(a, name, Value::Float(if v == NONE { u32::MAX as f32 } else { v as f32 }));
    }
    attributes::set(a, "UncompressedTangentFrame", Value::Bool(i.uncompressed));
    attributes::set(a, "D_MORPH", Value::Combo(u8::from(morph)));
    let mut result = Ok(());
    for chunk in jobs.chunks(MAX_INSTANCES) {
        attributes::set(a, "InstanceCount", Value::Float(chunk.len() as f32));
        attributes::set_constant_buffer(a, StringToken::of("Instances_t"), &instances(chunk, skin.weights));
        result = result.and(compute::gpu::dispatch(gpu, shader, &[a], compute::gpu::Work::Threads([skin.vertices, chunk.len() as u32, 1])));
    }
    objects::destroy(a);
    result
}

/// Ints managed passes as `int*` with `n` elements.
fn ints(p: RawPtr<c_void>, n: usize) -> Vec<u32> {
    if p.is_null() {
        return vec![0; n];
    }
    // SAFETY: managed passes pinned spans of nInstances ints (RenderContext.DispatchSkinning).
    unsafe { p.cast::<i32>().as_slice(n) }.iter().map(|v| v.max(&0).unsigned_abs()).collect()
}

/// `DispatchModelMeshSkinning`; deformation volumes are not applied (TODO).
#[allow(clippy::too_many_arguments)]
fn dispatch(mode: material_mode::Ref, m: crate::model::Ref, mesh: i32, transforms: buffers::Handle, slots: RawPtr<c_void>, offsets: RawPtr<c_void>, n: i32, morph: bool) -> bool {
    let (Some(model), Some(transforms)) = (crate::model::get(m), buffers::get(transforms)) else { return false };
    let Some(skin) = usize::try_from(mesh).ok().and_then(|i| super::mesh_skin(&model, m, i)) else { return false };
    let n = n.max(0) as usize;
    let jobs: Vec<Job> = ints(slots, n).into_iter().zip(ints(offsets, n)).map(|(slot, offset)| Job { slot, offset }).collect();
    let shader = material_mode::material(mode).map_or_else(|| SHADER.to_owned(), |m| m.shader().to_owned());
    run(&shader, &skin, &transforms, &jobs, morph).inspect_err(|e| compute::warn_once(&shader, e)).is_ok()
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::DISPATCH_MODEL_MESH_SKINNING, |_c, mode, m, mesh, transforms, slots, offsets, n, _weights, _volumes, _vo, _vc, _anchors, _ao, morph| {
        dispatch(mode, m, mesh, transforms, slots, offsets, n, morph)
    });
}
