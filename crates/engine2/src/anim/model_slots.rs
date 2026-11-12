//! `CModel`'s skeleton, attachment, hitbox and animation queries, and the `CAttachment`,
//! `CHitBoxSet` and `CHitBox` objects they hand out.

use super::conv::{to_rot, to_transform, to_v3};
use super::data::{self, Part};
use abi::slots::{c_attachment as att, c_hit_box as hb, c_hit_box_set as set, c_model as m};
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use anim::model::Hitbox;
use std::ffi::{CStr, c_void};
use std::sync::Arc;

type Model = NativeRef<class::CModel>;

fn idx(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

pub(super) fn bone_name(model: Model, i: i32) -> StrRet {
    let d = data::of(model);
    StrRet::from(idx(i).and_then(|i| d.anim.skeleton.bone(i)).map(|b| b.name.clone()))
}

pub(super) fn bone_parent(model: Model, i: i32) -> i32 {
    let d = data::of(model);
    idx(i).and_then(|i| d.anim.skeleton.bone(i)).and_then(|b| b.parent).map_or(-1, |p| p as i32)
}

fn find_bone(model: Model, name: Option<&CStr>) -> i32 {
    let name = name.map(CStr::to_string_lossy).unwrap_or_default();
    data::of(model).anim.skeleton.find(&name).map_or(-1, |i| i as i32)
}

/// Bind pose in model space ("relative to the root bone", Model.Skeleton.cs).
pub(super) fn bone_transform(model: Model, i: i32) -> abi::structs::Transform {
    let d = data::of(model);
    let s = &d.anim.skeleton;
    let bones = s.model_space(&s.bind_pose());
    to_transform(&idx(i).and_then(|i| bones.get(i).copied()).unwrap_or_default())
}

fn local_bone(model: Model, i: i32) -> anim::Xform {
    data::of(model).anim.skeleton.bone(idx(i).unwrap_or(usize::MAX)).map(|b| b.local).unwrap_or_default()
}

pub(super) fn attachment_transform(model: Model, name: StringToken, out: &mut abi::structs::Transform) -> bool {
    let d = data::of(model);
    let Some(a) = d.anim.attachments.iter().find(|a| StringToken::of(&a.name) == name) else { return false };
    *out = to_transform(&d.anim.bind_attachment(a));
    true
}

fn sequence_names(model: Model, out: NativeRef<class::CUtlVector_CUtlString_>) {
    let d = data::of(model);
    let names: Vec<String> = d.anim.clips.iter().filter(|c| !c.hidden).map(|c| c.name.clone()).chain(d.anim.clip_names.iter().cloned()).collect();
    crate::containers::with(out, |v: &mut Vec<String>| v.extend(names));
}

fn part<C>(p: *mut c_void) -> Option<Arc<Part>> {
    objects::get::<C, Part>(NativeRef::from_raw(p))
}

fn on_attachment<R: Default>(p: *mut c_void, f: impl FnOnce(&anim::model::Attachment) -> R) -> R {
    part::<class::CAttachment>(p).and_then(|p| p.model.anim.attachments.get(p.index).map(f)).unwrap_or_default()
}

fn on_hitbox<R: Default>(p: *mut c_void, f: impl FnOnce(&Hitbox) -> R) -> R {
    part::<class::CHitBox>(p).and_then(|p| p.model.anim.hitbox_sets.get(p.set)?.hitboxes.get(p.index).map(f)).unwrap_or_default()
}

fn influence<R: Default>(a: NativeRef<class::CAttachment>, i: i32, f: impl FnOnce(&anim::model::Influence) -> R) -> R {
    on_attachment(a.as_raw(), |a| idx(i).and_then(|i| a.influences.get(i)).map(f).unwrap_or_default())
}

fn set_name(p: *mut c_void) -> StrRet {
    StrRet::from(part::<class::CHitBoxSet>(p).and_then(|p| p.model.anim.hitbox_sets.get(p.set).map(|s| s.name.clone())))
}

pub fn register(r: &mut Registry) {
    bind!(r, m::NUM_BONES, |model| data::of(model).anim.skeleton.len() as i32);
    bind!(r, m::FIND_BONE_INDEX, find_bone);
    bind!(r, m::BONE_NAME, bone_name);
    bind!(r, m::BONE_PARENT, bone_parent);
    bind!(r, m::BONE_FLAGS, |model, i| data::of(model).anim.skeleton.bone(idx(i).unwrap_or(usize::MAX)).map_or(0, |b| b.flags));
    bind!(r, m::GET_BONE_TRANSFORM, bone_transform);
    bind!(r, m::BONE_POS_PARENT_SPACE, |model, i| to_v3(local_bone(model, i).pos));
    bind!(r, m::BONE_ROT_PARENT_SPACE, |model, i| to_rot(local_bone(model, i).rot));
    // A model names a skeleton only when it references a `.vnmskel`; no core model does.
    bind!(r, m::GET_SKELETON_NAME, |_| StrRet::from(String::new()));

    bind!(r, m::GET_NUM_ATTACHMENTS, |model| data::of(model).anim.attachments.len() as i32);
    bind!(r, m::GET_ATTACHMENT, |model, i| idx(i).map(|i| data::of(model).attachment_ref(i)).unwrap_or_default());
    bind!(r, m::GET_ATTACHMENT_NAME_FROM_INDEX, |model, i| StrRet::from(idx(i).and_then(|i| data::of(model).anim.attachments.get(i).map(|a| a.name.clone()))));
    bind!(r, m::GET_ATTACHMENT_TRANSFORM, attachment_transform);
    bind!(r, att::GET_M_NAME, |p| StrRet::from(on_attachment(p, |a| Some(a.name.clone()))));
    bind!(r, att::GET_M_N_INFLUENCES, |p| on_attachment(p, |a| a.influences.len().min(u8::MAX as usize) as u8));
    bind!(r, att::GET_M_B_IGNORE_ROTATION, |p| on_attachment(p, |a| a.ignore_rotation));
    bind!(r, att::GET_INFLUENCE_NAME, |a, i| StrRet::from(influence(a, i, |inf| Some(inf.bone.clone()))));
    bind!(r, att::GET_INFLUENCE_OFFSET, |a, i| to_v3(influence(a, i, |inf| inf.offset.pos)));
    bind!(r, att::GET_INFLUENCE_ROTATION, |a, i| to_rot(influence(a, i, |inf| inf.offset.rot)));

    // Hitbox sets are per mesh in Source 2; ours are model-wide, so every mesh index sees them.
    bind!(r, m::GET_HITBOX_SET_COUNT, |model, _mesh| data::of(model).anim.hitbox_sets.len() as i32);
    bind!(r, m::GET_HITBOX_SET_BY_INDEX, |model, i, _mesh| idx(i).map(|i| data::of(model).set_ref(i)).unwrap_or_default());
    bind!(r, m::FIND_HITBOX_SET_INDEX_BY_NAME, |model, name, _mesh| {
        let name = name.map(CStr::to_string_lossy).unwrap_or_default();
        data::of(model).anim.hitbox_sets.iter().position(|s| s.name.eq_ignore_ascii_case(&name)).map_or(-1, |i| i as i32)
    });
    bind!(r, m::FIND_HITBOX_SET_BY_NAME, |model, name, _mesh| {
        let name = name.map(CStr::to_string_lossy).unwrap_or_default();
        let d = data::of(model);
        d.anim.hitbox_sets.iter().position(|s| s.name.eq_ignore_ascii_case(&name)).map(|i| d.set_ref(i)).unwrap_or_default()
    });
    bind!(r, m::GET_BONE_INDEX_FOR_HITBOX, |model, h| {
        let bone = on_hitbox(h.as_raw(), |h| h.bone.clone());
        data::of(model).anim.skeleton.find(&bone).map_or(-1, |i| i as i32)
    });
    bind!(r, set::NUMHITBOXES, |s| part::<class::CHitBoxSet>(s.as_raw()).and_then(|p| p.model.anim.hitbox_sets.get(p.set).map(|s| s.hitboxes.len() as i32)).unwrap_or(0));
    bind!(r, set::P_HITBOX, |s, i| part::<class::CHitBoxSet>(s.as_raw()).zip(idx(i)).map(|(p, i)| p.model.hitbox_ref(p.set, i)).unwrap_or_default());
    bind!(r, set::GET_M_NAME, set_name);
    bind!(r, hb::GET_M_NAME, |p| StrRet::from(on_hitbox(p, |h| Some(h.name.clone()))));
    bind!(r, hb::GET_M_S_SURFACE_PROPERTY, |p| StrRet::from(on_hitbox(p, |h| Some(h.surface.clone()))));
    bind!(r, hb::GET_M_S_BONE_NAME, |p| StrRet::from(on_hitbox(p, |h| Some(h.bone.clone()))));
    bind!(r, hb::GET_M_N_BONE_NAME_HASH, |p| on_hitbox(p, |h| StringToken::of(&h.bone).0));
    bind!(r, hb::GET_M_N_SHAPE_TYPE, |p| on_hitbox(p, |h| h.shape.kind()));
    bind!(r, hb::GET_M_V_MIN_BOUNDS, |p| to_v3(on_hitbox(p, |h| h.shape.fields().0)));
    bind!(r, hb::GET_M_V_MAX_BOUNDS, |p| to_v3(on_hitbox(p, |h| h.shape.fields().1)));
    bind!(r, hb::GET_M_FL_SHAPE_RADIUS, |p| on_hitbox(p, |h| h.shape.fields().2));
    bind!(r, hb::GET_M_B_TRANSLATION_ONLY, |p| on_hitbox(p, |h| h.translation_only));
    bind!(r, hb::GET_M_N_HIT_BOX_INDEX, |p| part::<class::CHitBox>(p).map_or(0, |p| p.index as u16));
    bind!(r, hb::GET_TAG, |h, i| StrRet::from(on_hitbox(h.as_raw(), |h| idx(i).and_then(|i| h.tags.get(i).cloned()))));

    bind!(r, m::GET_NUM_ANIM, |model| data::of(model).anim.animation_names().count() as i32);
    bind!(r, m::GET_ANIMATION_NAME, |model, i| StrRet::from(idx(i).and_then(|i| data::of(model).anim.animation_names().nth(i).map(str::to_owned))));
    bind!(r, m::GET_SEQUENCE_NAMES, sequence_names);
    bind!(r, m::GET_ANIMATION_GRAPH_NAME, |model| StrRet::from(data::of(model).anim.graph.clone()));
    bind!(r, m::GET_NUM_WEIGHTLISTS, |_| 0);
}
