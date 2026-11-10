//! Runtime skeletal models: `CModelBuilder`'s bones, attachments, hitboxes and animation groups
//! (`CAnimationGroupBuilder`), registered as the built model's animation data.
// Meshes, physics and materials: see `crate::model::builder`.

use super::conv::{quat, v3, xform};
use super::data;
use abi::slots::{c_animation_group_builder as ag, c_builder_hit_box as hb, c_model_builder as mb};
use abi::structs::Transform;
use abi::{NativeRef, Registry, bind, class};
use anim::model::{Attachment, Hitbox, HitboxSet, HitboxShape, Influence, ModelAnim};
use anim::{Clip, Xform};
use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Builder {
    anim: ModelAnim,
    /// Attachments in model space with their parent bone, placed when the model is made.
    attachments: Vec<(String, String, Xform)>,
}

type Shared = Arc<Mutex<Builder>>;
type Ref = NativeRef<class::CModelBuilder>;

/// A builder hitbox: its builder, set and index.
struct HitboxRef(Shared, usize, usize);

struct Group(Mutex<Vec<Clip>>);

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn on<R: Default>(b: Ref, f: impl FnOnce(&mut Builder) -> R) -> R {
    objects::get::<_, Mutex<Builder>>(b).map(|b| f(&mut b.lock().unwrap())).unwrap_or_default()
}

pub(super) fn add_bone(b: Ref, name: Option<&CStr>, parent: Option<&CStr>, pos: abi::structs::Vector3, rot: abi::structs::Rotation, _bounds: abi::structs::BBox) {
    let (name, parent) = (text(name), text(parent));
    on(b, |b| {
        if let Err(e) = b.anim.skeleton.push_model_space(&name, &parent, Xform::new(v3(pos), quat(rot))) {
            abi::rt::log::warn(&format!("ModelBuilder.AddBone: {e:?}"));
        }
    });
}

pub(super) fn add_hitbox(b: Ref, set: Option<&CStr>, name: Option<&CStr>) -> NativeRef<class::CBuilderHitBox> {
    let Some(shared) = objects::get::<_, Mutex<Builder>>(b) else { return NativeRef::NULL };
    let (set, name) = (text(set), text(name));
    let (s, i) = {
        let mut b = shared.lock().unwrap();
        let sets = &mut b.anim.hitbox_sets;
        let s = sets.iter().position(|x| x.name == set).unwrap_or_else(|| {
            sets.push(HitboxSet { name: set, hitboxes: Vec::new() });
            sets.len() - 1
        });
        let shape = HitboxShape::Box { min: glam::Vec3::ZERO, max: glam::Vec3::ZERO };
        sets[s].hitboxes.push(Hitbox { name, bone: String::new(), surface: String::new(), tags: Vec::new(), shape, translation_only: false });
        (s, sets[s].hitboxes.len() - 1)
    };
    objects::create(HitboxRef(shared, s, i))
}

pub(super) fn on_hitbox(h: NativeRef<class::CBuilderHitBox>, f: impl FnOnce(&mut Hitbox)) {
    let Some(h) = objects::get::<_, HitboxRef>(h) else { return };
    if let Some(x) = h.0.lock().unwrap().anim.hitbox_sets.get_mut(h.1).and_then(|s| s.hitboxes.get_mut(h.2)) {
        f(x);
    }
}

pub(super) fn create_model(b: Ref, name: Option<&CStr>, anonymous: bool) -> NativeRef<class::CModel> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = Some(text(name)).filter(|n| !n.is_empty() && !anonymous).unwrap_or_else(|| format!("__runtime_model_{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    let anim = on(b, |b| {
        let mut anim = b.anim.clone();
        let bind = anim.skeleton.model_space(&anim.skeleton.bind_pose());
        for (att, parent, model) in &b.attachments {
            let offset = anim.skeleton.find(parent).map_or(*model, |p| bind[p].to_local(model));
            let influences = vec![Influence { bone: parent.clone(), offset, weight: 1.0 }];
            anim.attachments.push(Attachment { name: att.clone(), influences, ignore_rotation: false });
        }
        anim
    });
    data::insert(&name, anim);
    crate::model::create_runtime(&name, crate::model::builder::take(b))
}

pub(super) fn on_clip(g: NativeRef<class::CAnimationGroupBuilder>, i: i32, f: impl FnOnce(&mut Clip)) {
    let Some(g) = objects::get::<_, Group>(g) else { return };
    let mut clips = g.0.lock().unwrap();
    if let Some(c) = usize::try_from(i).ok().and_then(|i| clips.get_mut(i)) {
        f(c);
    }
}

pub(super) fn add_frame(g: NativeRef<class::CAnimationGroupBuilder>, i: i32, bones: abi::RawPtr<std::ffi::c_void>, n: i32) {
    // SAFETY: managed passes a pinned `Transform[n]` (ModelBuilder.Animation.cs, `fixed`).
    let bones = unsafe { bones.cast::<Transform>().as_slice(n.max(0) as usize) };
    let frame: Vec<Xform> = bones.iter().map(|t| xform(*t)).collect();
    on_clip(g, i, |c| c.frames.push(frame));
}

pub(super) fn new_builder() -> Ref {
    objects::create(Mutex::new(Builder::default()))
}

pub(super) fn add_attachment(b: Ref, name: Option<&CStr>, parent: Option<&CStr>, pos: abi::structs::Vector3, rot: abi::structs::Rotation) {
    let entry = (text(name), text(parent), Xform::new(v3(pos), quat(rot)));
    on(b, |b| b.attachments.push(entry));
}

pub(super) fn add_group(b: Ref, g: NativeRef<class::CAnimationGroupBuilder>) {
    let clips = objects::get::<_, Group>(g).map(|g| g.0.lock().unwrap().clone()).unwrap_or_default();
    on(b, |b| b.anim.clips.extend(clips.into_iter().map(Arc::new)));
}

pub(super) fn new_group() -> NativeRef<class::CAnimationGroupBuilder> {
    objects::create(Group(Mutex::new(Vec::new())))
}

pub(super) fn add_animation(g: NativeRef<class::CAnimationGroupBuilder>) -> i32 {
    objects::get::<_, Group>(g).map_or(-1, |g| {
        let mut clips = g.0.lock().unwrap();
        clips.push(Clip::new("", 30.0));
        clips.len() as i32 - 1
    })
}

pub fn register(r: &mut Registry) {
    bind!(r, mb::CREATE, new_builder);
    bind!(r, mb::DELETE_THIS, |b| {
        crate::model::builder::forget(b);
        objects::destroy(b);
    });
    bind!(r, mb::ADD_RUN_TIME_BONE, add_bone);
    bind!(r, mb::ADD_RUN_TIME_ATTACHMENT, add_attachment);
    bind!(r, mb::ADD_HIT_BOX, add_hitbox);
    bind!(r, mb::ADD_ANIMATION_GROUP_RUNTIME, add_group);
    bind!(r, mb::CREATE_RUNTIME_MODEL, create_model);

    bind!(r, hb::SET_BONE_NAME, |h, s| on_hitbox(h, |x| x.bone = text(s)));
    bind!(r, hb::SET_SURFACE, |h, s| on_hitbox(h, |x| x.surface = text(s)));
    bind!(r, hb::ADD_TAG, |h, s| on_hitbox(h, |x| x.tags.push(text(s))));
    bind!(r, hb::SET_BOX, |h, min, max| on_hitbox(h, |x| x.shape = HitboxShape::Box { min: v3(min), max: v3(max) }));
    bind!(r, hb::SET_SPHERE, |h, c, radius| on_hitbox(h, |x| x.shape = HitboxShape::Sphere { center: v3(c), radius }));
    bind!(r, hb::SET_CAPSULE, |h, a, b, radius| on_hitbox(h, |x| x.shape = HitboxShape::Capsule { a: v3(a), b: v3(b), radius }));

    bind!(r, ag::CREATE, new_group);
    bind!(r, ag::DELETE_THIS, |g| {
        objects::destroy(g);
    });
    bind!(r, ag::ADD_ANIMATION, add_animation);
    bind!(r, ag::ADD_FRAME, add_frame);
    bind!(r, ag::SET_NAME, |g, i, name| on_clip(g, i, |c| c.name = text(name)));
    bind!(r, ag::SET_FRAME_RATE, |g, i, fps| on_clip(g, i, |c| c.fps = fps));
    bind!(r, ag::SET_LOOPING, |g, i, v| on_clip(g, i, |c| c.looping = v));
    bind!(r, ag::SET_DELTA, |g, i, v| on_clip(g, i, |c| c.delta = v));
    bind!(r, ag::SET_DISABLE_INTERPOLATION, |g, i, v| on_clip(g, i, |c| c.interpolate = !v));
}
