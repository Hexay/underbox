//! `CAnimClipResource` and `CAnimSkeletonResource` (managed `AnimationClip`/`AnimationSkeleton`)
//! over a model's animation data, loaded by `Glue_Resources_GetAnimClip`/`GetAnimSkeleton`.
//! A skeleton's name is its model's path; a clip's is `<model>.vmdl/<clip>`, or a bare clip name
//! (or file stem) found among loaded models.

use super::conv::to_transform;
use super::data::{self, ModelData};
use crate::math::identity;
use crate::resource::{self, Resource};
use abi::slots::{c_anim_clip_resource as c, c_anim_skeleton_resource as s, glue_resources as g};
use abi::structs::Transform;
use crate::bind_resource_handle;
use abi::{NativeRef, Registry, StrRet, bind, class};
use anim::{Clip, Pose};
use std::ffi::CStr;
use std::sync::Arc;

pub struct SkeletonResource {
    pub name: String,
    pub model: Arc<ModelData>,
}

pub struct ClipResource {
    pub skeleton: SkeletonResource,
    pub clip: Arc<Clip>,
}

impl Resource for SkeletonResource {}
impl Resource for ClipResource {}

type SkeletonRef = NativeRef<class::CAnimSkeletonResource>;
type ClipRef = NativeRef<class::CAnimClipResource>;

pub fn skeleton(name: &str) -> Option<SkeletonResource> {
    let model = data::by_name(name);
    (!model.anim.skeleton.is_empty()).then(|| SkeletonResource { name: name.to_owned(), model })
}

pub fn clip(name: &str) -> Option<ClipResource> {
    let lower = name.to_ascii_lowercase();
    let found = match lower.find(".vmdl") {
        Some(i) => {
            let (model, clip) = (&name[..i + 5], name[i + 5..].trim_start_matches(['/', '\\', ':']));
            Some((model.to_owned(), data::by_name(model), clip.to_owned()))
        }
        None => {
            let stem = std::path::Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
            data::find_clip(stem).map(|(model, d)| (model, d, stem.to_owned()))
        }
    };
    let (model_name, model, clip_name) = found?;
    let clip = model.anim.clip(&clip_name)?.clone();
    Some(ClipResource { skeleton: SkeletonResource { name: model_name, model }, clip })
}

fn with_skeleton<R: Default>(h: SkeletonRef, f: impl FnOnce(&SkeletonResource) -> R) -> R {
    resource::resolve::<_, SkeletonResource>(h).map(|r| f(&r)).unwrap_or_default()
}

fn with_clip<R: Default>(h: ClipRef, f: impl FnOnce(&ClipResource) -> R) -> R {
    resource::resolve::<_, ClipResource>(h).map(|r| f(&r)).unwrap_or_default()
}

fn bone_at(bones: &[anim::Xform], i: i32) -> Transform {
    usize::try_from(i).ok().and_then(|i| bones.get(i)).map_or_else(identity, to_transform)
}

fn name(n: Option<&CStr>) -> String {
    n.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, g::GET_ANIM_SKELETON, |n| skeleton(&name(n)).map_or(NativeRef::NULL, |s| resource::new_handle(Arc::new(s))));
    bind!(r, g::GET_ANIM_CLIP, |n| clip(&name(n)).map_or(NativeRef::NULL, |c| resource::new_handle(Arc::new(c))));

    bind_resource_handle!(r, s, SkeletonResource);
    let bones = |h: SkeletonRef| with_skeleton(h, |s| s.model.anim.skeleton.len() as i32);
    bind!(r, s::GET_NUM_BONES, bones);
    bind!(r, s::GET_LOW_LOD_BONE_COUNT, bones);
    bind!(r, s::GET_BONE_NAME, |h, i| StrRet::from(with_skeleton(h, |s| s.model.anim.skeleton.bone(i as usize).map(|b| b.name.clone()))));
    bind!(r, s::GET_PARENT_BONE_INDEX, |h, i| {
        with_skeleton(h, |s| s.model.anim.skeleton.bone(i as usize).map_or(-1, |b| b.parent.map_or(-1, |p| p as i32)))
    });
    bind!(r, s::GET_BONE_PARENT_SPACE_TRANSFORM_S2, |h, i| with_skeleton(h, |s| Some(bone_at(&s.model.anim.skeleton.bind_pose(), i))).unwrap_or_else(identity));
    bind!(r, s::GET_BONE_MODEL_SPACE_TRANSFORM_S2, |h, i| {
        with_skeleton(h, |s| {
            let k = &s.model.anim.skeleton;
            Some(bone_at(&k.model_space(&k.bind_pose()), i))
        })
        .unwrap_or_else(identity)
    });
    bind!(r, s::IS_PROP_SKELETON, |h| with_skeleton(h, |s| s.model.anim.skeleton.len() <= 1));
    bind!(r, s::HAS_SYNTHETIC_ROOT_BONE, |_| false);
    bind!(r, s::GET_NUM_SECONDARY_SKELETONS, |_| 0);
    bind!(r, s::GET_SECONDARY_SKELETON_NAME, |_, _| StrRet::from(None::<String>));
    bind!(r, s::GET_NUM_MASK_DEFINITIONS, |_| 0);
    bind!(r, s::GET_MASK_DEFINITION_NAME, |_, _| StrRet::from(None::<String>));

    bind_resource_handle!(r, c, ClipResource);
    bind!(r, c::GET_NUM_FRAMES, |h| with_clip(h, |c| c.clip.frames.len() as i32));
    bind!(r, c::GET_DURATION, |h| with_clip(h, |c| c.clip.duration()));
    bind!(r, c::GET_FPS, |h| with_clip(h, |c| c.clip.fps));
    bind!(r, c::IS_ADDITIVE, |h| with_clip(h, |c| c.clip.delta));
    bind!(r, c::GET_NUM_TRACKS, |h| with_clip(h, |c| c.skeleton.model.anim.skeleton.len() as i32));
    // TODO: clip events and sync events (no source format carries them yet).
    bind!(r, c::GET_NUM_SYNC_EVENTS, |_| 0);
    bind!(r, c::GET_NUM_EVENTS, |_| 0);
    bind!(r, c::GET_EVENT_NAME, |_, _| StrRet::from(None::<String>));
    bind!(r, c::GET_EVENT_DATA, |_, _, _, _| NativeRef::NULL);
    bind!(r, c::GET_SKELETON_NAME, |h| StrRet::from(with_clip(h, |c| Some(c.skeleton.name.clone()))));
    bind!(r, c::IS_BOUND, |h| with_clip(h, |c| !c.skeleton.model.anim.skeleton.is_empty()));
    bind!(r, c::GET_BONE_PARENT_SPACE_TRANSFORM_S2, |h, time, i| {
        with_clip(h, |c| Some(bone_at(&c.clip.sample(time, &Pose::bind(&c.skeleton.model.anim.skeleton)).0, i))).unwrap_or_else(identity)
    });
    bind!(r, c::GET_ROOT_MOTION_TRANSFORM_S2, |h, time| with_clip(h, |c| Some(to_transform(&c.clip.root_motion(0.0, time)))).unwrap_or_else(identity));
    bind!(r, c::GET_AVERAGE_LINEAR_VELOCITY, |h| {
        with_clip(h, |c| {
            let d = c.clip.duration();
            if d > 0.0 { c.clip.root_motion(0.0, d).pos.length() / d } else { 0.0 }
        })
    });
}
