//! `CSceneAnimatableObject` update and bone slots: the per-frame sequence managed drives
//! (Update → bone writes → CalculateWorldSpaceBones → FinishUpdate) and the bone reads.

use super::callback;
use super::conv::{to_transform, v3, xform};
use super::object::{self, Ref, object_xform, with};
use abi::slots::c_scene_animatable_object as o;
use abi::structs::Transform;
use abi::{RawPtr, Registry, bind};
use anim::{Skeleton, Xform};
use std::ffi::{CStr, c_void};

fn idx(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

/// Owed callbacks after (re)attaching the model's graphs; run with no lock held.
pub fn settle(r: Ref) {
    let (made, changed) = object::attach_defaults(r);
    let (on_controller, on_graph1) = with(r, |s| (s.on_controller, s.on_graph1));
    if changed {
        callback::graph1_changed(on_graph1, r.as_raw());
    }
    if made {
        callback::controller_created(on_controller);
    }
}

fn update(r: Ref, dt: f32) {
    settle(r);
    let character = object_xform(r);
    with(r, |s| s.update(dt, character));
}

fn bone_by_name(r: Ref, name: Option<&CStr>) -> Option<usize> {
    let name = name?.to_string_lossy();
    with(r, |s| s.skeleton().find(&name))
}

fn world(r: Ref, i: Option<usize>) -> Transform {
    to_transform(&with(r, |s| i.and_then(|i| s.world.get(i).copied())).unwrap_or_else(|| object_xform(r)))
}

fn previous(r: Ref, i: Option<usize>) -> Transform {
    to_transform(&with(r, |s| i.and_then(|i| s.prev_world.get(i).copied())).unwrap_or_else(|| object_xform(r)))
}

/// Relative to the object (model space).
fn local(r: Ref, i: Option<usize>) -> Transform {
    let object = object_xform(r);
    to_transform(&with(r, |s| i.and_then(|i| s.world.get(i)).map(|w| object.to_local(w))).unwrap_or_default())
}

/// Copy up to `n` of `bones` into managed's `Transform[n]` at `out`.
fn write(out: RawPtr<c_void>, n: i32, bones: &[Xform]) {
    // SAFETY: managed passes a pinned `Transform[n]` (SceneModel.cs, `fixed`), live for the call.
    let out = unsafe { out.cast::<Transform>().as_mut_slice(n.max(0) as usize) };
    for (o, b) in out.iter_mut().zip(bones) {
        *o = to_transform(b);
    }
}

fn set_world(r: Ref, i: i32, t: Transform) {
    with(r, |s| {
        let Some(i) = idx(i).filter(|&i| i < s.world.len()) else { return };
        s.world[i] = xform(t);
        s.world_set.insert(i, xform(t));
    });
}

fn merge_from(r: Ref, parent: Ref) {
    let bones = with(parent, |p| p.named_world());
    let object = object_xform(r);
    with(r, |s| s.merge_from(&bones, &object));
}

fn attachment(r: Ref, name: abi::StringToken, world: bool, out: &mut Transform) -> bool {
    let object = object_xform(r);
    let found = with(r, |s| {
        let a = s.data.anim.attachments.iter().find(|a| abi::StringToken::of(&a.name) == name)?;
        let bone = |n: &str| s.skeleton().find(n).and_then(|i| s.world.get(i)).map(|w| if world { *w } else { object.to_local(w) });
        // Bone-less models: attachment offsets are from the object itself.
        let root = if world { object } else { Xform::IDENTITY };
        Some(a.resolve(|n| bone(n).or(Some(root))))
    });
    found.inspect(|x| *out = to_transform(x)).is_some()
}

pub fn register(r: &mut Registry) {
    bind!(r, o::UPDATE, update);
    bind!(r, o::SET_BIND_POSE, |r| with(r, |s| s.local = s.skeleton().bind_pose()));
    bind!(r, o::CALCULATE_WORLD_SPACE_BONES, |r| {
        let object = object_xform(r);
        with(r, |s| s.calculate_world(&object));
    });
    bind!(r, o::FINISH_UPDATE, |_| {});
    bind!(r, o::MERGE_FROM, merge_from);
    bind!(r, o::GET_M_FL_DELTA_TIME, |p| with(Ref::from_raw(p), |s| s.dt));
    // TODO: real's are the posed bounds; these are the model's render bounds (docs/gaps.md).
    bind!(r, o::GET_M_LOCAL_BOUNDS, |p| crate::scene::object::with(Ref::from_raw(p).cast(), |s| crate::model::bounds(s.model)));
    bind!(r, o::GET_M_WORLD_BOUNDS, |p| crate::scene::object::with(Ref::from_raw(p).cast(), |s| s.world_bounds()));

    bind!(r, o::SET_WORLD_SPACE_RENDER_BONE_TRANSFORM, set_world);
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_TRANSFORM, |r, i| world(r, idx(i)));
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_TRANSFORM_1, |r, name| world(r, bone_by_name(r, name)));
    bind!(r, o::GET_WORLD_SPACE_ANIMATION_TRANSFORM, |r, i| world(r, idx(i)));
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_PREVIOUS_TRANSFORM, |r, i| previous(r, idx(i)));
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_PREVIOUS_TRANSFORM_1, |r, name| previous(r, bone_by_name(r, name)));
    bind!(r, o::GET_LOCAL_SPACE_RENDER_BONE_TRANSFORM, |r, i| local(r, idx(i)));
    bind!(r, o::GET_LOCAL_SPACE_RENDER_BONE_TRANSFORM_1, |r, name| local(r, bone_by_name(r, name)));
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_TRANSFORMS, |r, n, out| with(r, |s| write(out, n, &s.world)));
    bind!(r, o::GET_WORLD_SPACE_RENDER_BONE_PREVIOUS_TRANSFORMS, |r, n, out| with(r, |s| write(out, n, &s.prev_world)));
    bind!(r, o::GET_PARENT_SPACE_BONES, |r, n, out| with(r, |s| write(out, n, &s.local)));
    bind!(r, o::GET_PARENT_SPACE_BONE, |r, i| to_transform(&with(r, |s| idx(i).and_then(|i| s.local.get(i).copied())).unwrap_or_default()));
    bind!(r, o::SET_PARENT_SPACE_BONE, |r, i, t| with(r, |s| {
        if let Some(b) = idx(i).and_then(|i| s.local.get_mut(i)) {
            *b = xform(t);
        }
    }));
    bind!(r, o::GET_CLOSEST_BONE_INDEX, |r, p| with(r, |s| Skeleton::closest(&s.world, v3(p)).map_or(-1, |i| i as i32)));

    bind!(r, o::CLEAR_PHYSICS_BONES, |r| with(r, |s| s.physics.clear()));
    bind!(r, o::SET_PHYSICS_BONE, |r, i, t| with(r, |s| {
        s.physics.insert(i as usize, xform(t));
    }));
    bind!(r, o::HAS_PHYSICS_BONES, |r| with(r, |s| !s.physics.is_empty()));
    bind!(r, o::GET_ROOT_MOTION, |r| to_transform(&with(r, |s| s.root_motion)));
    bind!(r, o::S_BOX_GET_ATTACHMENT, attachment);
}
