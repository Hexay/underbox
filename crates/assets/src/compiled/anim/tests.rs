use super::encode;
use super::{Animation, Key, Rig, read, read_group, sequences};
use crate::compiled::fixture;
use ::kv::Value;

fn object(v: &Value) -> &::kv::Object {
    v.as_object().unwrap()
}

fn near(a: &Key, b: &Key) -> bool {
    let dot: f32 = a.rotation.iter().zip(&b.rotation).map(|(x, y)| x * y).sum();
    a.position.iter().zip(&b.position).all(|(x, y)| (x - y).abs() < 1e-2) && (dot.abs() - 1.0).abs() < 1e-5 && (a.scale - b.scale).abs() < 1e-6
}

fn assert_same(got: &Animation, want: &Animation) {
    assert_eq!((&got.name, got.fps, got.looping, got.delta, got.hidden), (&want.name, want.fps, want.looping, want.delta, want.hidden));
    assert_eq!(got.frames.len(), want.frames.len());
    for (f, (g, w)) in got.frames.iter().zip(&want.frames).enumerate() {
        for (b, (gk, wk)) in g.iter().zip(w).enumerate() {
            assert!(near(gk, wk), "{} frame {f} bone {b}: {gk:?} != {wk:?}", want.name);
        }
    }
    assert_eq!(got.morphs, want.morphs);
}

#[test]
fn animations_round_trip_across_frame_blocks() {
    let (bones, flex) = (fixture::rig_bones(), fixture::flex_names());
    let rig = Rig { bones: &bones, flex: &flex };
    let anims = fixture::animations();
    for block in [1, 2, 64] {
        let (data, key) = encode::write(&anims, &rig, block);
        let got = read(object(&data), object(&key), &rig);
        assert_eq!(got.len(), anims.len());
        got.iter().zip(&anims).for_each(|(g, w)| assert_same(g, w));
    }
}

#[test]
fn channels_bind_by_name_so_bone_order_may_differ() {
    let (bones, flex) = (fixture::rig_bones(), fixture::flex_names());
    let anims = fixture::animations();
    let (data, key) = encode::write(&anims, &Rig { bones: &bones, flex: &flex }, 2);
    let reversed: Vec<_> = bones.iter().rev().cloned().collect();
    let got = read(object(&data), object(&key), &Rig { bones: &reversed, flex: &[] });
    let last = bones.len() - 1;
    for (f, frame) in got[0].frames.iter().enumerate() {
        assert!(near(&frame[0], &anims[0].frames[f][last]), "frame {f}");
    }
    assert!(got[0].morphs.is_empty(), "no controllers to bind the morph channel to");
}

#[test]
fn sequences_rename_and_reflag() {
    let anims = fixture::animations();
    let seq = encode::sequences(&[("@wave_loop", "wave", true, false), ("idle_seq", "@wave_loop", false, true)]);
    let got = sequences(object(&seq), anims.clone());
    let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["@wave_loop", "idle_seq", "wave", "nod"], "animations keep their own entries too");
    assert!(got[0].looping && !got[0].hidden && got[1].hidden);
    assert_eq!(got[1].frames, anims[0].frames, "a sequence can play another sequence");
}

#[test]
fn groups_hold_animations_inline_or_in_vanim_files() {
    let (bones, flex) = (fixture::rig_bones(), fixture::flex_names());
    let rig = Rig { bones: &bones, flex: &flex };
    let anims = fixture::animations();
    let (data, key) = encode::write(&anims, &rig, 2);
    let inline = read_group(&encode::group_file(&key, Some(&data), &[]), &rig, &|_| None).unwrap();
    assert_eq!(inline.len(), 2);
    let vanim = encode::anim_file(&data);
    let load = |p: &str| (p == "animation/wave.vanim_c").then(|| vanim.clone());
    let external = read_group(&encode::group_file(&key, None, &["animation/wave.vanim", "animation/missing.vanim"]), &rig, &load).unwrap();
    assert_eq!(external, inline);
}
