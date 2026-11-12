//! A puppet for render tests: the rigged model's bones without a ragdoll (so managed poses it
//! from animation alone), side vertices at mid height blended between root and spine, and one
//! non-looping clip that settles on a known pose. The morphed variant adds the rigged model's
//! morph set, with "smile" ramping to 1 over the clip (research/resources.md, "Morph sets").

use super::super::anim::{Animation, Key};
use super::super::model::{Model, ModelMesh};
use super::super::morph;
use super::super::texture::{Format, Texture};
use super::super::vbib::Packing;
use super::{HALF, controllers, cube, rig_bones, rules, targets};

pub const PUPPET: &str = "models/compiled/puppet.vmdl";
pub const PUPPET_MORPHED: &str = "models/compiled/puppet_morphed.vmdl";
pub const PUPPET_ATLAS: &str = "models/compiled/puppet_morphed/body_vmorf.vtex";
/// The spine's final turn about z in "bow", degrees.
pub const BOW_DEGREES: f32 = 60.0;
/// The head's final lift in "bow".
pub const BOW_HEAD_LIFT: f32 = 6.0;

fn about_z(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() / 2.0;
    [0.0, 0.0, h.sin(), h.cos()]
}

/// "bow": 5 frames at 10 fps, not looping; the spine turns and the head lifts (and with
/// `smile`, the smile morph ramps to 1).
pub fn bow(smile: bool) -> Animation {
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
    let morphs = if smile { (0..5).map(|f| vec![f as f32 * 0.25, 0.0]).collect() } else { Vec::new() };
    Animation { name: "bow".into(), fps: 10.0, looping: false, frames, morphs, ..Animation::default() }
}

fn vertex_count() -> usize {
    cube(HALF * 0.5).vertices.len()
}

/// The rigged cube's bones; the top face follows the head, the bottom the root, and the sides'
/// top edge is half spine, half root.
fn puppet(name: &str, morphed: bool) -> Model {
    let mut mesh = cube(HALF * 0.5);
    for (i, v) in mesh.vertices.iter_mut().enumerate() {
        let up = (v.position[2] / (2.0 * HALF)).clamp(0.0, 1.0);
        (v.bones, v.weights) = match i {
            20.. => ([2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
            _ if up > 0.0 => ([1, 0, 0, 0], [up, 1.0 - up, 0.0, 0.0]),
            _ => ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        };
    }
    let morph = morphed.then(|| morph::encode(&targets(), vertex_count(), controllers(), rules(), PUPPET_ATLAS).0);
    Model {
        name: name.into(),
        meshes: vec![ModelMesh { name: "body".into(), mesh, group_mask: 1, lod_mask: u64::MAX, morph, targets: Vec::new() }],
        mesh_groups: vec!["body".into()],
        default_group_mask: 1,
        bones: rig_bones(),
        animations: vec![bow(morphed)],
        ..Model::default()
    }
}

pub fn puppet_model() -> Model {
    puppet(PUPPET, false)
}

pub fn puppet_morphed_model() -> Model {
    puppet(PUPPET_MORPHED, true)
}

/// (compiled path, bytes) of both puppets and the morph atlas; their material is the crate's
/// (see `crate_files`).
pub fn puppet_files() -> Vec<(String, Vec<u8>)> {
    let (_, w, h, px) = morph::encode(&targets(), vertex_count(), controllers(), rules(), PUPPET_ATLAS);
    let atlas = Texture { width: w as u16, height: h as u16, depth: 1, format: Format::RGBA8888, levels: vec![px], ..Default::default() };
    vec![
        (format!("{PUPPET}_c"), super::super::model::write(&puppet_model(), Packing::Meshopt)),
        (format!("{PUPPET_MORPHED}_c"), super::super::model::write(&puppet_morphed_model(), Packing::Meshopt)),
        (format!("{PUPPET_ATLAS}_c"), super::super::texture::write(&atlas, false)),
    ]
}
