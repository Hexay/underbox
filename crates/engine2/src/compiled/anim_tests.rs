//! The rigged fixture model from a game folder: animation data through `anim::data`, sampled
//! at a known pose, and its key values as game data.

use super::tests::game_dir;
use anim::Pose;
use assets::compiled::fixture::{self, RIGGED};
use glam::{Quat, Vec3};

#[test]
fn compiled_animation_samples_a_known_pose() {
    game_dir();
    let data = crate::anim::data::by_name(RIGGED);
    let a = &data.anim;
    let names: Vec<&str> = a.animation_names().collect();
    assert_eq!(names, ["wave", "nod", "stretch"]);
    assert_eq!((a.flex.as_slice(), a.graph.as_deref()), (fixture::flex_names().as_slice(), Some("animgraphs/rigged.vanmgrph")));
    let wave = a.clip("wave").unwrap();
    // 0.25 s at 10 fps is halfway between frames 2 and 3: spine at 50 degrees, head lifted 1.25.
    let pose = wave.sample(0.25, &Pose::bind(&a.skeleton));
    let world = a.skeleton.model_space(&pose.0);
    let spin = Quat::from_rotation_z(50f32.to_radians());
    assert!(world[1].rot.angle_between(spin) < 1e-3, "{:?}", world[1].rot);
    let head_z = fixture::HALF + fixture::HALF * 0.5 + 1.25;
    assert!(world[2].pos.abs_diff_eq(Vec3::new(0.0, 0.0, head_z), 1e-2), "{:?}", world[2].pos);
    assert!((world[2].scale - 1.625).abs() < 1e-4);
    let eyes = a.attachment("eyes").unwrap().resolve(|n| a.skeleton.find(n).map(|i| world[i]));
    let want = Vec3::new(0.0, 0.0, head_z) + spin * Vec3::new(2.0 * 1.625, 0.0, 1.625);
    assert!(eyes.pos.abs_diff_eq(want, 2e-2), "{:?} != {want:?}", eyes.pos);
    assert_eq!(wave.morphs_at(0.25), Some(vec![0.625, 0.0]));
    assert_eq!(a.hitbox_sets[0].hitboxes.iter().map(|h| h.shape.kind()).collect::<Vec<_>>(), [0, 2]);
}

#[test]
fn compiled_key_values_are_game_data() {
    game_dir();
    let r = crate::model::load(Some(&std::ffi::CString::new(RIGGED).unwrap()), abi::Guid::default());
    let m = crate::model::get(r).unwrap();
    let prop = m.content.game_data.as_ref().and_then(|d| d.member("prop_data")).expect("prop_data");
    assert!(prop.member("health").is_some());
    let mesh = &m.content.meshes[0].mesh;
    assert_eq!(mesh.vertices().len(), 24);
    let morphs = mesh.state().morphs.clone();
    assert_eq!(morphs.iter().map(|x| (x.name.as_str(), x.deltas.len())).collect::<Vec<_>>(), [("smile", 4), ("blink", 1)]);
    assert!((morphs[0].deltas[0].position[2] - fixture::SMILE_LIFT).abs() < 1e-2);
}
