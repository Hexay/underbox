//! Deformation volumes (managed's `ModelDeformer`): an ordered stack of masked model-space
//! operations (inflate, squash/stretch, transform, suck, project inside a sphere, box, capsule or
//! everywhere) that skinning_cs applies after morphs and before bones (`Deformation.hlsl`). Each
//! is a 208-byte `SceneDeformationVolumeData`. With anchors (a model-space point per bone, for
//! `DeformationMode.Rigid`) a vertex instead moves as the stack moves its bones' anchors.
//!
//! Managed's renderer packs its own volume and anchor buffers and passes them to
//! `DispatchModelMeshSkinning`; engine-drawn objects keep what `SetDeformationVolumes` and
//! `SetDeformationAnchors` stored on the object ([`crate::anim::object::Deformers`]).

use crate::anim::object::{Deformers, with};
use crate::render::device;
use abi::slots::c_scene_animatable_object as o;
use abi::{Registry, bind};
use std::sync::Arc;

/// `sizeof(SceneDeformationVolumeData)`: 13 float4s.
pub const VOLUME_BYTES: usize = 208;

/// The buffers one deformed dispatch reads: volumes, and anchors after an unused first one.
pub struct Deformation {
    pub volumes: Arc<render::Buffer>,
    pub anchors: Arc<render::Buffer>,
}

/// What an engine-drawn object with `bones` mesh bones (the model's, in order) is deformed by:
/// the buffers, its volume count and its first anchor (0 when not rigid). `None` undeformed.
pub fn native(d: &Deformers, bones: usize) -> Option<(Deformation, u32, u32)> {
    let count = d.volumes.len() / VOLUME_BYTES;
    if count == 0 {
        return None;
    }
    let gpu = device::gpu()?;
    let mut anchors = vec![[0f32; 4]];
    if !d.anchors.is_empty() {
        anchors.extend((0..bones).map(|b| d.anchors.get(b).copied().unwrap_or_default()));
    }
    let bytes: Vec<u8> = anchors.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
    let storage = |label, data: &[u8]| Arc::new(render::Buffer::with_data(gpu, label, render::BufferKind::Storage, data));
    let deformation = Deformation { volumes: storage("deformation volumes", &d.volumes[..count * VOLUME_BYTES]), anchors: storage("deformation anchors", &bytes) };
    Some((deformation, count as u32, u32::from(!d.anchors.is_empty())))
}

pub fn register(r: &mut Registry) {
    bind!(r, o::SET_DEFORMATION_VOLUMES, |r, n, p| {
        let n = usize::try_from(n).unwrap_or(0);
        // SAFETY: managed passes n pinned SceneDeformationVolumeData (SceneModel.SetDeformationVolumes).
        let bytes = if p.is_null() || n == 0 { Vec::new() } else { unsafe { p.cast::<u8>().as_slice(n * VOLUME_BYTES) }.to_vec() };
        with(r, |s| s.deformers.volumes = bytes);
    });
    bind!(r, o::SET_DEFORMATION_ANCHORS, |r, n, p| {
        let n = usize::try_from(n).unwrap_or(0);
        // SAFETY: managed passes n pinned Vector4s (SceneModel.SetDeformationAnchors); only 4-byte aligned.
        let anchors = if p.is_null() || n == 0 { Vec::new() } else { unsafe { p.cast::<[f32; 4]>().as_slice(n) }.to_vec() };
        with(r, |s| s.deformers.anchors = anchors);
    });
}
