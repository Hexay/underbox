//! Runtime skeletal models through the builder slots, then model queries, sequence playback and
//! an AnimGraph2 driven through its controller. Scene objects need managed's handle table, so
//! object state is exercised directly rather than through the object slots.

use super::conv::{to_rot, v3};
use super::object::AnimState;
use super::{builder, controller, data, graph, model_slots};
use crate::math::vec3;
use abi::structs::{BBox, Transform};
use abi::{NativeRef, RawPtr, StrRet, StringToken, class};
use anim::graph::{Definition, PoseNode, State, Transition, ValueNode};
use anim::params::{ParamDef, Value};
use anim::Xform;
use glam::{Quat, Vec3};
use std::f32::consts::FRAC_PI_2;
use std::sync::{Arc, Mutex};

fn transform(pos: Vec3, rot: Quat) -> Transform {
    super::conv::to_transform(&Xform::new(pos, rot))
}

/// `root` at z = 10 and `arm` 5 further along X (model space); a `hand` attachment 2 past the arm;
/// an `HB_arm` capsule; and `wave`, a 1 fps looping clip turning `root` 90° about Z over 1 s.
fn build(name: &std::ffi::CStr) -> NativeRef<class::CModel> {
    let identity = to_rot(Quat::IDENTITY);
    let b = builder::new_builder();
    builder::add_bone(b, Some(c"root"), None, vec3(0.0, 0.0, 10.0), identity, BBox::default());
    builder::add_bone(b, Some(c"arm"), Some(c"root"), vec3(5.0, 0.0, 10.0), identity, BBox::default());
    builder::add_attachment(b, Some(c"hand"), Some(c"arm"), vec3(7.0, 0.0, 10.0), identity);
    let hb = builder::add_hitbox(b, Some(c"default"), Some(c"HB_arm"));
    builder::on_hitbox(hb, |h| h.bone = "arm".into());

    let g = builder::new_group();
    let a = builder::add_animation(g);
    builder::on_clip(g, a, |c| {
        c.name = "wave".into();
        c.fps = 1.0;
        c.looping = true;
    });
    let arm = transform(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY);
    for turn in [0.0, FRAC_PI_2] {
        let mut frame = [transform(Vec3::new(0.0, 0.0, 10.0), Quat::from_rotation_z(turn)), arm];
        builder::add_frame(g, a, RawPtr::from_raw(frame.as_mut_ptr().cast()), 2);
    }
    builder::add_group(b, g);
    builder::create_model(b, Some(name), false)
}

fn text(s: StrRet) -> String {
    match s {
        StrRet::Owned(c) => c.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

#[test]
fn builder_model_answers_bone_attachment_and_hitbox_queries() {
    let model = build(c"anim_test_queries");
    let d = data::of(model);
    assert_eq!(d.anim.skeleton.len(), 2);
    assert_eq!(text(model_slots::bone_name(model, 1)), "arm");
    assert_eq!(model_slots::bone_parent(model, 1), 0);
    let arm = model_slots::bone_transform(model, 1);
    assert!(v3(arm.position).abs_diff_eq(Vec3::new(5.0, 0.0, 10.0), 1e-5), "bind pose in model space");
    let mut hand = Transform::default();
    assert!(model_slots::attachment_transform(model, StringToken::of("HAND"), &mut hand));
    assert!(v3(hand.position).abs_diff_eq(Vec3::new(7.0, 0.0, 10.0), 1e-5));
    assert!(!model_slots::attachment_transform(model, StringToken::of("nope"), &mut hand));
    assert_eq!(d.anim.hitbox_sets[0].hitboxes[0].bone, "arm");
    assert_eq!(d.anim.clips[0].frames.len(), 2);
}

#[test]
fn sequence_playback_moves_world_bones() {
    let model = build(c"anim_test_sequence");
    let mut s = AnimState::new(model);
    s.sequence.play("wave");
    let object = Xform::new(Vec3::new(100.0, 0.0, 0.0), Quat::IDENTITY);
    s.calculate_world(&object);
    assert!(s.world[1].pos.abs_diff_eq(Vec3::new(105.0, 0.0, 10.0), 1e-4), "bind pose before any update");
    s.update(0.5);
    s.calculate_world(&object);
    let h = 5.0 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(s.world[1].pos.abs_diff_eq(Vec3::new(100.0 + h, h, 10.0), 1e-3), "45° through the turn: {:?}", s.world[1].pos);
    assert!(s.prev_world[1].pos.abs_diff_eq(Vec3::new(105.0, 0.0, 10.0), 1e-4), "previous frame kept for velocity");
    s.rate = 2.0;
    s.update(0.125);
    s.calculate_world(&object);
    let a = 0.75 * FRAC_PI_2;
    assert!(s.world[1].pos.abs_diff_eq(Vec3::new(100.0 + 5.0 * a.cos(), 5.0 * a.sin(), 10.0), 1e-3), "playback rate doubles dt");

    let mut child = AnimState::new(model);
    child.merge_from(&s.named_world(), &object);
    assert!(child.world[1].pos.abs_diff_eq(s.world[1].pos, 1e-5), "bone merge copies same-named bones");
}

#[test]
fn graph_parameters_drive_state_transitions() {
    let model = build(c"anim_test_graph");
    let mut def = Definition::default();
    def.params.push(ParamDef::new("b_wave", Value::Bool(false)));
    let b_wave = def.add_value(ValueNode::Param(0));
    let rest = def.add_pose(PoseNode::Reference);
    let slot = def.slot("wave");
    let wave = def.add_pose(PoseNode::Clip { slot, looping: true, speed: 1.0 });
    let states = vec![
        State { name: "Rest".into(), pose: rest, transitions: vec![Transition { to: 1, condition: b_wave, duration: 0.0 }] },
        State { name: "Wave".into(), pose: wave, transitions: vec![] },
    ];
    def.root = Some(def.add_pose(PoseNode::StateMachine { states, default: 0 }));
    let resource: graph::Ref2 = crate::resource::new_handle(Arc::new(graph::Graph2 { def: Some(def.into_shared()) }));

    let shared = Arc::new(Mutex::new(AnimState::new(model)));
    assert!(shared.lock().unwrap().set_graph2(resource, Arc::downgrade(&shared)));
    let ctl = shared.lock().unwrap().graph2.as_ref().unwrap().controller;
    assert!(controller::set(ctl, StringToken::of("b_wave"), Value::Bool(true)));
    assert!(!controller::set(ctl, StringToken::of("missing"), Value::Bool(true)));
    assert_eq!(controller::get(ctl, StringToken::of("B_WAVE")), Some(Value::Bool(true)));

    let mut s = shared.lock().unwrap();
    s.update(0.5);
    assert!(s.local[0].rot.abs_diff_eq(Quat::IDENTITY, 1e-5), "Wave enters at its first frame");
    s.update(0.5);
    assert!(s.local[0].rot.abs_diff_eq(Quat::from_rotation_z(FRAC_PI_2 / 2.0), 1e-4), "then plays the clip");
}
