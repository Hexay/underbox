//! A rigged model with known contents: three bones, embedded animations plus an animation group
//! whose animation is in a `.vanim_c`, two morph targets driven by flex rules, attachments,
//! hitboxes and key values.

use super::super::anim::{Animation, Key, Rig, encode};
use super::super::flex::{Controller, Op, Rule};
use super::super::model::{Bone, Model, ModelMesh};
use super::super::morph::{self, Target};
use super::super::rig::{Attachment, Hitbox, HitboxSet, Influence};
use super::super::texture::{Format, Texture};
use super::super::vbib::Packing;
use super::{HALF, cube};

pub const RIGGED: &str = "models/compiled/rigged.vmdl";
pub const ATLAS: &str = "models/compiled/rigged_morphs.vtex";
pub const GROUP: &str = "models/compiled/rigged.vagrp";
pub const VANIM: &str = "models/compiled/rigged_stretch.vanim";
/// The spine's rotation about z at wave frame `f`, in degrees.
pub const WAVE_DEGREES: [f32; 5] = [0.0, 20.0, 40.0, 60.0, 80.0];
/// The head's offset from its bind position at wave frame `f`.
pub const WAVE_HEAD_LIFT: [f32; 5] = [0.0, 0.5, 1.0, 1.5, 2.0];
/// How far the smile target lifts the top face's vertices.
pub const SMILE_LIFT: f32 = 2.0;
pub const KEY_VALUES: &str = r#"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->
{
	prop_data = { base = "Wooden.Small" health = 25 }
	anim_graph_resource = "animgraphs/rigged.vanmgrph"
}"#;

fn bone(name: &str, parent: Option<usize>, position: [f32; 3]) -> Bone {
    Bone { name: name.into(), parent, position, rotation: [0.0, 0.0, 0.0, 1.0] }
}

pub fn rig_bones() -> Vec<Bone> {
    vec![bone("root", None, [0.0; 3]), bone("spine", Some(0), [0.0, 0.0, HALF]), bone("head", Some(1), [0.0, 0.0, HALF * 0.5])]
}

pub fn flex_names() -> Vec<String> {
    vec!["smile".into(), "blink".into()]
}

fn about_z(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() / 2.0;
    [0.0, 0.0, h.sin(), h.cos()]
}

/// "wave" (5 frames, looping: spine turns, head lifts and grows, smile ramps) and "nod" (a
/// 3-frame delta tilting the head).
pub fn animations() -> Vec<Animation> {
    let bones = rig_bones();
    let bind: Vec<Key> = bones.iter().map(Key::bind).collect();
    let wave = (0..5)
        .map(|f| {
            let mut k = bind.clone();
            k[1].rotation = about_z(WAVE_DEGREES[f]);
            k[2].position[2] += WAVE_HEAD_LIFT[f];
            k[2].scale = 1.0 + f as f32 * 0.25;
            k
        })
        .collect();
    let smile = (0..5).map(|f| vec![f as f32 * 0.25, 0.0]).collect();
    let tilt = |d: f32| {
        let h = d.to_radians() / 2.0;
        Key { rotation: [h.sin(), 0.0, 0.0, h.cos()], ..Key::IDENTITY }
    };
    let nod = [0.0, 15.0, 0.0].iter().map(|&d| vec![Key::IDENTITY, Key::IDENTITY, tilt(d)]).collect();
    vec![
        Animation { name: "wave".into(), fps: 10.0, looping: true, delta: false, hidden: false, frames: wave, morphs: smile },
        Animation { name: "nod".into(), fps: 30.0, looping: false, delta: true, hidden: false, frames: nod, morphs: Vec::new() },
    ]
}

/// The group's animation: the root slides along x over 3 frames.
pub fn stretch() -> Animation {
    let bind: Vec<Key> = rig_bones().iter().map(Key::bind).collect();
    let frames = (0..3)
        .map(|f| {
            let mut k = bind.clone();
            k[0].position[0] = f as f32;
            k
        })
        .collect();
    Animation { name: "stretch".into(), fps: 15.0, frames, ..Animation::default() }
}

pub fn controllers() -> Vec<Controller> {
    flex_names().into_iter().map(|name| Controller { name, kind: "default".into(), min: 0.0, max: 1.0 }).collect()
}

/// smile = smile; blink = blink dominated by smile.
pub fn rules() -> Vec<Rule> {
    vec![Rule { flex: 0, ops: vec![Op::Fetch1(0)] }, Rule { flex: 1, ops: vec![Op::Fetch1(1), Op::Fetch1(0), Op::Dominate(1)] }]
}

/// Morph targets over the rigged mesh: smile lifts the top face, blink pulls vertex 0 in x.
pub fn targets() -> Vec<Target> {
    let smile = (20..24).map(|v| (v, [0.0, 0.0, SMILE_LIFT], [0.0; 3])).collect();
    vec![Target { name: "smile".into(), deltas: smile }, Target { name: "blink".into(), deltas: vec![(0, [-1.0, 0.0, 0.0], [0.0, 0.0, 0.5])] }]
}

pub fn attachments() -> Vec<Attachment> {
    let at = |name: &str, bone: &str, offset| Attachment { name: name.into(), influences: vec![Influence { bone: bone.into(), offset, rotation: about_z(90.0), weight: 1.0, root: false }], ignore_rotation: false };
    vec![at("eyes", "head", [2.0, 0.0, 1.0]), at("hand", "spine", [0.0, 4.0, 0.0])]
}

pub fn hitbox_sets() -> Vec<HitboxSet> {
    let hb = |name: &str, bone: &str, shape, min, max, radius| Hitbox { name: name.into(), surface: "flesh".into(), bone: bone.into(), min, max, radius, shape, translation_only: false, group: 1 };
    vec![HitboxSet { name: "default".into(), hitboxes: vec![hb("chest", "spine", 0, [-4.0; 3], [4.0; 3], 0.0), hb("skull", "head", 2, [0.0; 3], [0.0, 0.0, 4.0], 3.0)] }]
}

/// The rigged model as written; `missing`, the group's animation and targets come from reading.
pub fn rigged_model() -> Model {
    let mut mesh = cube(HALF * 0.5);
    for (i, v) in mesh.vertices.iter_mut().enumerate() {
        v.bones = [if i >= 20 { 2 } else { 1 }, 0, 0, 0];
    }
    let vertices = mesh.vertices.len();
    let (set, ..) = morph::encode(&targets(), vertices, controllers(), rules(), ATLAS);
    Model {
        name: RIGGED.into(),
        meshes: vec![ModelMesh { name: "body".into(), mesh, group_mask: 1, lod_mask: u64::MAX, morph: Some(set), targets: Vec::new() }],
        mesh_groups: vec!["body".into()],
        default_group_mask: 1,
        bones: rig_bones(),
        key_values: KEY_VALUES.into(),
        attachments: attachments(),
        hitbox_sets: hitbox_sets(),
        anim_groups: vec![GROUP.into()],
        animations: animations(),
        ..Model::default()
    }
}

fn atlas() -> Texture {
    let (_, w, h, px) = morph::encode(&targets(), cube(HALF * 0.5).vertices.len(), controllers(), rules(), ATLAS);
    Texture { width: w as u16, height: h as u16, depth: 1, format: Format::RGBA8888, levels: vec![px], ..Texture::default() }
}

/// (compiled path, bytes) of the rigged model, its morph atlas, animation group and `.vanim`;
/// its material is the crate's (see `crate_files`).
pub fn rigged_files() -> Vec<(String, Vec<u8>)> {
    let (bones, flex) = (rig_bones(), flex_names());
    let (data, key) = encode::write(&[stretch()], &Rig { bones: &bones, flex: &flex }, 2);
    vec![
        (format!("{RIGGED}_c"), super::super::model::write(&rigged_model(), Packing::Raw)),
        (format!("{ATLAS}_c"), super::super::texture::write(&atlas(), false)),
        (format!("{GROUP}_c"), encode::group_file(&key, None, &[VANIM])),
        (format!("{VANIM}_c"), encode::anim_file(&data)),
    ]
}
