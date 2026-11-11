//! Skinning for engine-drawn objects (the native render path): the object's world-space bones
//! become a transform buffer laid out as managed's (`TransformBuffer.AddSkinned`), skinning_cs
//! writes each skinned mesh into the vertex cache once per frame, and the mesh draws with that
//! buffer as its `g_TransformBuffer` (base entry at slot 0).

use super::{Job, MeshSkin, cache, dispatch, morphs, rows};
use crate::handles::Kind;
use crate::render::attributes::{self, Value};
use crate::render::buffers;
use crate::render::draw::morph::Weights;
use crate::scene::object;
use glam::{Mat4, Vec4};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// An animatable object's world-space bones, read before its state is locked.
pub struct Pose(Vec<Mat4>);

pub fn pose(r: object::Ref) -> Option<Pose> {
    let animatable = object::get(r).is_some_and(|o| o.kind == Kind::SceneAnimatableObject);
    animatable.then(|| Pose(crate::anim::object::with(r.cast(), |s| s.world.iter().map(super::xform_matrix).collect())))
}

/// One 64-byte `TransformBufferData_t`.
fn entry(rows: [Vec4; 3], alpha: f32, tint: u32, cache: u32, weights: u32) -> Vec<u8> {
    let mut out: Vec<u8> = rows.iter().flat_map(|r| r.to_array()).flat_map(f32::to_le_bytes).collect();
    out.extend(alpha.to_le_bytes());
    [tint, cache, weights].iter().for_each(|w| out.extend(w.to_le_bytes()));
    out
}

fn pack(tint: [f32; 4]) -> u32 {
    let [r, g, b, _] = tint.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u32);
    (r << 16) | (g << 8) | b
}

/// Base, morph and bone entries (`AddSkinned`, `SetMorphSubrect`, `SetVertexCacheOffset`).
pub fn entries(bones: &[Mat4], tint: [f32; 4], offset: u32, weights: u32, subrect: Option<[f32; 4]>) -> Vec<u8> {
    let (alpha, rgb) = (tint[3], pack(tint));
    let mut out = entry([Vec4::new(0.0, 0.0, 1.0, 1.0), Vec4::ZERO, Vec4::ZERO], alpha, rgb, offset, weights);
    let [sx, sy, sz, sw] = subrect.unwrap_or_default();
    out.extend(entry([Vec4::new(0.0, 0.0, sx, sy), Vec4::new(0.0, 0.0, 0.0, sz), Vec4::new(0.0, 0.0, 0.0, sw)], 0.0, 0, 0, 0));
    for (i, b) in bones.iter().enumerate() {
        let extra = if i == 0 { (alpha, rgb, offset, weights) } else { (0.0, 0, 0, 0) };
        out.extend(entry(rows(*b), extra.0, extra.1, extra.2, extra.3));
    }
    out
}

#[derive(Default)]
struct Frame {
    drawn: HashMap<(usize, usize), attributes::Ref>,
    handles: Vec<buffers::Handle>,
}

static FRAME: Mutex<Option<Frame>> = Mutex::new(None);

/// A new frame: last frame's skinned meshes and their buffers are released.
pub fn begin_frame() {
    let Some(f) = FRAME.lock().unwrap().take() else { return };
    f.drawn.into_values().for_each(|a| {
        objects::destroy(a);
    });
    f.handles.into_iter().for_each(buffers::destroy);
}

/// Mesh `index` of object `key`, skinned this frame (the first ask dispatches): the attributes
/// its draw binds. `None` when skinning fails; the draw then stays rigid.
#[allow(clippy::too_many_arguments)]
pub fn skinned(key: usize, index: usize, skin: &MeshSkin, model: crate::model::Ref, pose: &Pose, tint: [f32; 4], morph: Option<(&Weights, &crate::mesh::render_mesh::RenderMesh)>) -> Option<attributes::Ref> {
    if let Some(a) = FRAME.lock().unwrap().as_ref().and_then(|f| f.drawn.get(&(key, index)).copied()) {
        return Some(a);
    }
    let gpu = crate::render::device::gpu()?;
    let inverse = super::inverse_bind_poses(model);
    let bones: Vec<Mat4> = inverse.iter().enumerate().map(|(b, inv)| pose.0.get(b).copied().unwrap_or(Mat4::IDENTITY) * *inv).collect();
    let offset = cache::allocate(skin.vertices)?;    let composite = morph.filter(|(w, m)| morphs::active(m, w)).map(|(w, m)| morphs::compose(&[(w, m)]));
    let subrect = composite.as_ref().and_then(|(_, s)| s.first().copied());
    let morph_buffer = composite.and_then(|(data, _)| morphs::upload(&data));
    let transforms = Arc::new(render::Buffer::with_data(gpu, "skinned transforms", render::BufferKind::Storage, &entries(&bones, tint, offset, skin.weights, subrect)));
    dispatch::run(dispatch::SHADER, skin, &transforms, &[Job { slot: 0, offset }], morph_buffer.as_ref()).inspect_err(|e| crate::render::compute::warn_once(dispatch::SHADER, e)).ok()?;
    let handle = buffers::insert(transforms);
    let a = attributes::create();
    attributes::set(a, "g_TransformBuffer", Value::Buffer(handle.value));
    let mut frame = FRAME.lock().unwrap();
    let f = frame.get_or_insert_with(Frame::default);
    f.handles.push(handle);
    f.drawn.insert((key, index), a);
    Some(a)
}
