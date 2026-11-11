//! A compiled model's animation data (`assets::compiled::model::read_rig`) as the anim crate's
//! `ModelAnim`: skeleton, attachments, hitboxes, clips with their morph tracks, flex controller
//! names and the graph it names.

use anim::model::{Attachment, Hitbox, HitboxSet, HitboxShape, Influence};
use anim::{Clip, ModelAnim, Xform};
use assets::compiled::{anim as canim, model, rig};
use glam::{Quat, Vec3};
use std::sync::Arc;

fn quat(q: [f32; 4]) -> Quat {
    let q = Quat::from_array(q);
    if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY }
}

fn attachment(a: &rig::Attachment) -> Attachment {
    let influences = a.influences.iter().map(|i| Influence { bone: i.bone.clone(), offset: Xform::new(Vec3::from(i.offset), quat(i.rotation)), weight: i.weight }).collect();
    Attachment { name: a.name.clone(), influences, ignore_rotation: a.ignore_rotation }
}

fn hitbox(h: &rig::Hitbox) -> Hitbox {
    let (min, max) = (Vec3::from(h.min), Vec3::from(h.max));
    let shape = match h.shape {
        1 => HitboxShape::Sphere { center: min, radius: h.radius },
        2 => HitboxShape::Capsule { a: min, b: max, radius: h.radius },
        _ => HitboxShape::Box { min, max },
    };
    Hitbox { name: h.name.clone(), bone: h.bone.clone(), surface: h.surface.clone(), tags: Vec::new(), shape, translation_only: h.translation_only }
}

pub fn clip(a: &canim::Animation) -> Clip {
    let key = |k: &canim::Key| Xform { pos: Vec3::from(k.position), rot: quat(k.rotation), scale: k.scale };
    Clip {
        frames: a.frames.iter().map(|f| f.iter().map(key).collect()).collect(),
        looping: a.looping,
        delta: a.delta,
        hidden: a.hidden,
        morphs: a.morphs.clone(),
        ..Clip::new(&a.name, a.fps)
    }
}

pub fn model_anim(m: &model::Model) -> ModelAnim {
    ModelAnim {
        skeleton: crate::model::compiled::skeleton(m),
        attachments: m.attachments.iter().map(attachment).collect(),
        hitbox_sets: m.hitbox_sets.iter().map(|s| HitboxSet { name: s.name.clone(), hitboxes: s.hitboxes.iter().map(hitbox).collect() }).collect(),
        clips: m.animations.iter().map(|a| Arc::new(clip(a))).collect(),
        clip_names: Vec::new(),
        graph: m.anim_graph(),
        flex: m.flex_controllers().iter().map(|c| c.name.clone()).collect(),
    }
}
