//! A puppet for render tests: the rigged model's bones and morph targets, no ragdoll (so managed
//! poses it from animation alone), side vertices at mid height blended between root and spine,
//! and one non-looping clip that settles on a known pose.

use super::super::anim::{Animation, Key};
use super::super::model::{Model, ModelMesh};
use super::super::morph;
use super::super::vbib::Packing;
use super::{HALF, cube, rig_bones};

pub const PUPPET: &str = "models/compiled/puppet.vmdl";
pub const PUPPET_ATLAS: &str = "models/compiled/puppet_morphs.vtex";
/// The spine's final turn about z in "bow", degrees.
pub const BOW_DEGREES: f32 = 60.0;
/// The head's final lift in "bow".
pub const BOW_HEAD_LIFT: f32 = 6.0;

fn about_z(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() / 2.0;
    [0.0, 0.0, h.sin(), h.cos()]
}

/// "bow": 5 frames at 10 fps, not looping; the spine turns, the head lifts, smile ramps to 1.
pub fn bow() -> Animation {
    let bind: Vec<Key> = rig_bones().iter().map(Key::bind).collect();
    let frames = (0..5)
        .map(|f| {
            let t = f as f32 / 4.0;
            let mut k = bind.clone();
            k[1].rotation = about_z(BOW_DEGREES * t);
            k[2].position[2] += BOW_HEAD_LIFT * t;
            k
        })
        .collect();
    let smile = (0..5).map(|f| vec![f as f32 * 0.25, 0.0]).collect();
    Animation { name: "bow".into(), fps: 10.0, looping: false, frames, morphs: smile, ..Animation::default() }
}

/// The rigged cube's bones and morphs; the top face follows the head, the bottom the root, and
/// the sides' top edge is half spine, half root.
pub fn puppet_model() -> Model {
    let mut mesh = cube(HALF * 0.5);
    for (i, v) in mesh.vertices.iter_mut().enumerate() {
        let up = (v.position[2] / (2.0 * HALF)).clamp(0.0, 1.0);
        (v.bones, v.weights) = match i {
            20.. => ([2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
            _ if up > 0.0 => ([1, 0, 0, 0], [up, 1.0 - up, 0.0, 0.0]),
            _ => ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        };
    }
    let vertices = mesh.vertices.len();
    let (set, ..) = morph::encode(&super::targets(), vertices, super::controllers(), super::rules(), PUPPET_ATLAS);
    Model {
        name: PUPPET.into(),
        meshes: vec![ModelMesh { name: "body".into(), mesh, group_mask: 1, lod_mask: u64::MAX, morph: Some(set), targets: Vec::new() }],
        mesh_groups: vec!["body".into()],
        default_group_mask: 1,
        bones: rig_bones(),
        animations: vec![bow()],
        ..Model::default()
    }
}

/// (compiled path, bytes) of the puppet and its morph atlas; its material is the crate's (see
/// `crate_files`).
pub fn puppet_files() -> Vec<(String, Vec<u8>)> {
    let (_, w, h, px) = morph::encode(&super::targets(), cube(HALF * 0.5).vertices.len(), super::controllers(), super::rules(), PUPPET_ATLAS);
    let atlas = super::super::texture::Texture { width: w as u16, height: h as u16, depth: 1, format: super::super::texture::Format::RGBA8888, levels: vec![px], ..Default::default() };
    vec![(format!("{PUPPET}_c"), super::super::model::write(&puppet_model(), Packing::Meshopt)), (format!("{PUPPET_ATLAS}_c"), super::super::texture::write(&atlas, false))]
}
