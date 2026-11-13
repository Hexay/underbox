//! Skinning and morphs on the GPU for both render paths. Native's model (CSceneSystem's compute
//! skinning, mirrored by managed's `MeshRenderFeature.Skinning.cs`):
//!
//! - an animated object writes a base transform entry, a morph entry and one entry per bone
//!   (world pose × inverse bind pose) into a transform buffer (`transform_buffer.fxc`);
//! - meshes with one blend weight are skinned in the vertex shader (`GetTransformMatrix(slot,
//!   bone)`); meshes with more, or with morphs, run `skinning_cs` from the mesh's own vertex
//!   stream into the vertex cache ([`cache`], 24 bytes per vertex), and their draws take the
//!   `D_CS_VERTEX_ANIMATION` combo: the vertex shader reads the cache at the entry's offset plus
//!   the vertex's `nVertexCacheIndex` stream (shading/vertex.rs);
//! - morphs are composited first ([`morphs`]): a GPU pass of additive quads into one texture,
//!   which skinning_cs samples through our `morph.fxc` `Morph()` at the morph entry's subrect.
//!
//! Managed drives this through the `RenderTools` slots here; engine-drawn objects through
//! [`native`]. A mesh is skinned when its model has more than one bone and its layout has bone
//! indices in a form skinning_cs reads (compiled meshes: `Layout::sbox_skinned_vertex`, drawn and
//! skinned from `Layout::skinned_draw_vertex`, real's stream).

pub mod cache;
mod dispatch;
pub mod morphs;
pub mod native;
#[cfg(test)]
mod tests;

use crate::mesh::buffers::VertexBuffer;
use crate::mesh::layout::{Layout, format};
use crate::model::Model;
use abi::slots::render_tools as tools;
use abi::{RawPtr, Registry, bind};
use glam::{Mat4, Vec4};
use std::ffi::c_void;
use std::sync::Arc;

pub use dispatch::{Job, run};

/// Where skinning_cs finds a mesh's attributes, in bytes into one vertex; `NONE` when absent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub stride: u32,
    pub normal: u32,
    pub tangent: u32,
    pub bone_index: u32,
    pub bone_weight: u32,
    /// Float normals (and tangents), not the packed tangent frame.
    pub uncompressed: bool,
}

pub const NONE: u32 = u32::MAX;

/// The combo a draw from the vertex cache takes (`instancing.fxc`; managed's `MeshRenderFeature`).
pub const VERTEX_ANIMATION: &str = "D_CS_VERTEX_ANIMATION";

/// What skinning a model mesh takes (`RenderTools.GetModelMeshSkinning`).
#[derive(Clone)]
pub struct MeshSkin {
    pub vertices: u32,
    pub bones: u32,
    /// The most weights any vertex uses: 1 skins in the vertex shader unless it morphs.
    pub weights: u32,
    pub input: Input,
    pub vb: Arc<VertexBuffer>,
}

fn field(layout: &Layout, semantic: &str, formats: &[u32]) -> Option<(u32, u32)> {
    layout.fields.iter().find(|f| f.semantic == semantic && f.index == 0 && formats.contains(&f.format) && f.offset % 4 == 0).map(|f| (f.offset as u32, f.format))
}

/// skinning_cs's view of a `layout` vertex: a float position first, 8-bit bone indices, optional
/// unorm weights.
fn input(layout: &Layout) -> Option<Input> {
    let (position, _) = field(layout, "position", &[format::RGB32F, format::RGBA32F])?;
    let (bone_index, _) = field(layout, "blendindices", &[format::RGBA8_UINT])?;
    let (normal, normal_format) = field(layout, "normal", &[format::RGB32F, format::RGBA32F, format::R32_UINT]).unwrap_or((NONE, 0));
    let uncompressed = normal_format != format::R32_UINT;
    let tangent = field(layout, "tangent", &[format::RGBA32F]).filter(|_| uncompressed).map_or(NONE, |f| f.0);
    let bone_weight = field(layout, "blendweight", &[format::RGBA8_UNORM]).map_or(NONE, |f| f.0);
    (position == 0 && normal != NONE).then_some(Input { stride: layout.stride as u32, normal, tangent, bone_index, bone_weight, uncompressed })
}

fn weights_used(vb: &VertexBuffer) -> u32 {
    let Some(at) = field(&vb.meta, "blendweight", &[format::RGBA8_UNORM]).map(|f| f.0 as usize) else { return 1 };
    let bytes = vb.read();
    bytes.chunks_exact(vb.stride().max(1)).map(|v| v[at..at + 4].iter().filter(|w| **w != 0).count() as u32).max().unwrap_or(1).max(1)
}

/// Mesh `mesh` of `model`'s skinning; `None` when it draws rigidly. `input` describes the stream
/// it draws from (gpu_mesh::draw_vertices), which skinning_cs reads too.
pub fn mesh_skin(model: &Model, handle: crate::model::Ref, mesh: usize) -> Option<MeshSkin> {
    let bones = crate::anim::data::of(handle).anim.skeleton.len() as u32;
    let vb = model.content.meshes.get(mesh)?.mesh.state().vertices.clone()?;
    let input = input(&crate::render::draw::gpu_mesh::draw_layout(&vb)).filter(|_| bones > 1)?;
    Some(MeshSkin { vertices: vb.count() as u32, bones, weights: weights_used(&vb), input, vb })
}

/// Rows of `m` as native's `matrix3x4_t`.
pub fn rows(m: Mat4) -> [Vec4; 3] {
    [m.row(0), m.row(1), m.row(2)]
}

pub fn xform_matrix(x: &anim::Xform) -> Mat4 {
    Mat4::from_scale_rotation_translation(glam::Vec3::splat(x.scale), x.rot, x.pos)
}

/// Each bone's inverse model-space bind pose: mesh bones are the model's bones, in order.
pub fn inverse_bind_poses(handle: crate::model::Ref) -> Vec<Mat4> {
    let data = crate::anim::data::of(handle);
    let s = &data.anim.skeleton;
    s.model_space(&s.bind_pose()).iter().map(|x| xform_matrix(x).inverse()).collect()
}

fn skinning(m: crate::model::Ref, mesh: i32, bones: RawPtr<c_void>, weights: RawPtr<c_void>) -> i32 {
    let skin = crate::model::get(m).zip(usize::try_from(mesh).ok()).and_then(|(model, i)| mesh_skin(&model, m, i));
    let Some(skin) = skin else { return -1 };
    // SAFETY: managed passes two ints on its stack (RenderContext.ReadMeshSkin).
    unsafe {
        *bones.cast::<i32>().as_ptr() = skin.bones as i32;
        *weights.cast::<i32>().as_ptr() = skin.weights as i32;
    }
    skin.vertices as i32
}

fn skinning_bones(m: crate::model::Ref, mesh: i32, poses: RawPtr<c_void>, masters: RawPtr<c_void>, n: i32) -> bool {
    let inverse = inverse_bind_poses(m);
    if crate::model::get(m).zip(usize::try_from(mesh).ok()).and_then(|(model, i)| mesh_skin(&model, m, i)).is_none() || inverse.len() < n.max(0) as usize {
        return false;
    }
    let n = n.max(0) as usize;
    // SAFETY: managed passes pinned Matrix3x4[n] and int[n] (RenderContext.ReadMeshSkin).
    // Gotcha: only 4-byte aligned, so plain floats rather than glam's 16-byte Vec4.
    let (poses, masters) = unsafe { (poses.cast::<[[f32; 4]; 3]>().as_mut_slice(n), masters.cast::<i32>().as_mut_slice(n)) };
    for (b, (pose, master)) in poses.iter_mut().zip(masters.iter_mut()).enumerate() {
        (*pose, *master) = (rows(inverse[b]).map(|r| r.to_array()), b as i32);
    }
    true
}

/// A new frame: the vertex cache and morph composite start over and last frame's engine-drawn
/// skins are released.
pub fn begin_frame() {
    cache::begin_frame();
    morphs::begin_frame();
    native::begin_frame();
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::GET_MODEL_MESH_SKINNING, skinning);
    bind!(r, tools::GET_MODEL_MESH_SKINNING_BONES, skinning_bones);
    bind!(r, tools::ALLOCATE_VERTEX_CACHE, |n| cache::allocate(n.max(0) as u32).map_or(-1, |o| o as i32));
    bind!(r, tools::GET_VERTEX_CACHE_BUFFER, cache::handle);
    dispatch::register(r);
    morphs::register(r);
}
