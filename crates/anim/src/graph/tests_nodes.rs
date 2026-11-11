//! Blend2D, selectors, ease/spring and IK nodes evaluated through an instance.

use super::tests::{clip, clips, x};
use super::*;
use crate::params::Target;
use crate::pose::Pose;
use crate::skeleton::Skeleton;
use crate::xform::Xform;
use glam::{Quat, Vec3};

fn param(def: &mut Definition, name: &str, v: Value) -> ValueId {
    def.params.push(ParamDef::new(name, v));
    def.add_value(ValueNode::Param(def.params.len() - 1))
}

#[test]
fn blend2d_blends_the_triangle_under_the_parameters() {
    let mut def = Definition::default();
    let (px, py) = (param(&mut def, "x", Value::Float(0.5)), param(&mut def, "y", Value::Float(0.0)));
    let corners = [([0.0, 0.0], "idle"), ([1.0, 0.0], "walk"), ([1.0, 1.0], "run"), ([0.0, 1.0], "jump")];
    let points = corners.iter().map(|&(at, name)| (at, clip(&mut def, name, true))).collect();
    def.root = Some(def.add_pose(PoseNode::Blend2D { x: px, y: py, points, triangles: vec![[0, 1, 2], [0, 2, 3]] }));
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    assert!((x(&g.update(0.1, &bind)) - 5.0).abs() < 1e-3, "halfway idle → walk");
    g.params.set_by_name("y", Value::Float(0.5));
    g.params.set_by_name("x", Value::Float(1.0));
    assert!((x(&g.update(0.1, &bind)) - 20.0).abs() < 1e-3, "halfway walk → run");
    g.params.set_by_name("x", Value::Float(-3.0));
    assert!((x(&g.update(0.1, &bind)) - 50.0).abs() < 1e-3, "outside: the nearest edge, idle ↔ jump");
}

#[test]
fn selectors_choose_once_when_they_start() {
    let mut def = Definition::default();
    let run = param(&mut def, "run", Value::Bool(false));
    let seed = param(&mut def, "seed", Value::Float(0.6));
    let (walk, sprint, idle) = (clip(&mut def, "walk", true), clip(&mut def, "run", true), clip(&mut def, "idle", true));
    let selector = def.add_pose(PoseNode::Selector { cases: vec![(run, sprint)], fallback: walk });
    let options = vec![idle, clip(&mut def, "jump", true)];
    let variation = def.add_pose(PoseNode::Variation { seed, options, weights: Some(vec![1, 3]) });
    let half = def.add_value(ValueNode::Const(Value::Float(0.5)));
    def.root = Some(def.add_pose(PoseNode::Blend { base: selector, layer: variation, weight: half }));
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    assert!((x(&g.update(0.1, &bind)) - 55.0).abs() < 1e-3, "walk (10) and jump, seed 0.6 of weights 1:3 (100)");
    g.params.set_by_name("run", Value::Bool(true));
    g.params.set_by_name("seed", Value::Float(0.1));
    assert!((x(&g.update(0.1, &bind)) - 55.0).abs() < 1e-3, "kept until restarted");
    g.restart();
    assert!((x(&g.update(0.1, &bind)) - 15.0).abs() < 1e-3, "run (30) and idle");
}

#[test]
fn ease_and_spring_values_move_toward_their_input() {
    let mut def = Definition::default();
    let input = param(&mut def, "v", Value::Float(0.0));
    let ease = def.add_value(ValueNode::Ease { input, time: 1.0, easing: Easing::InOutQuad, start: None });
    let spring = def.add_value(ValueNode::Spring { input, hertz: 2.0, damping: 1.0, start: Some(5.0) });
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    g.update(0.1, &bind);
    assert!(g.value(spring).as_float() < 5.0 && g.value(spring).as_float() > 0.0, "starts at 5, heads to 0");
    g.params.set_by_name("v", Value::Float(10.0));
    g.update(0.25, &bind);
    assert!((g.value(ease).as_float() - 1.25).abs() < 1e-4, "InOutQuad at a quarter: 2·0.25² of the way");
    g.update(0.25, &bind);
    assert!((g.value(ease).as_float() - 5.0).abs() < 1e-4);
    for _ in 0..40 {
        g.update(0.05, &bind);
    }
    assert!((g.value(ease).as_float() - 10.0).abs() < 1e-4 && (g.value(spring).as_float() - 10.0).abs() < 0.05);
}

/// A machine flipping between a variation state, picking by `RandomFloat(0, 4)` as an index over
/// idle/walk/run/jump, and an idle state, on bool `go`. Returns each variation entry's pick.
fn variation_picks(g: &mut Instance, entries: usize) -> Vec<f32> {
    let bind = Pose::zero(1);
    let mut picks = Vec::new();
    for _ in 0..entries {
        g.params.set_by_name("go", Value::Bool(false));
        g.update(0.1, &bind);
        let pick = x(&g.update(0.1, &bind));
        assert_eq!(x(&g.update(0.1, &bind)), pick, "held while the state plays");
        picks.push(pick);
        g.params.set_by_name("go", Value::Bool(true));
        g.update(0.1, &bind);
    }
    picks
}

fn random_variation_graph() -> Arc<Definition> {
    let mut def = Definition::default();
    let go = param(&mut def, "go", Value::Bool(false));
    let stay = def.add_value(ValueNode::Not(go));
    let seed = def.add_value(ValueNode::Random { min: 0.0, max: 3.999 });
    let options = ["idle", "walk", "run", "jump"].map(|c| clip(&mut def, c, true)).to_vec();
    let pick = def.add_pose(PoseNode::Variation { seed, options, weights: None });
    let rest = clip(&mut def, "idle", true);
    let states = vec![
        State { name: "pick".into(), pose: pick, transitions: vec![Transition { to: 1, condition: go, duration: 0.0 }] },
        State { name: "rest".into(), pose: rest, transitions: vec![Transition { to: 0, condition: stay, duration: 0.0 }] },
    ];
    def.root = Some(def.add_pose(PoseNode::StateMachine { states, default: 1 }));
    def.into_shared()
}

#[test]
fn random_floats_redraw_each_time_their_state_starts() {
    let def = random_variation_graph();
    let mut g = Instance::new(def.clone(), clips);
    let picks = variation_picks(&mut g, 24);
    assert!(picks.iter().all(|p| [0.0, 10.0, 30.0, 100.0].contains(p)), "{picks:?}");
    let distinct = [0.0, 10.0, 30.0, 100.0].iter().filter(|o| picks.contains(o)).count();
    assert!(distinct >= 3, "the seed varies, so the variation does: {picks:?}");
    g.restart();
    assert_eq!(variation_picks(&mut g, 24), picks, "restart rewinds the instance's stream (recordings replay)");
    let mut other = Instance::new(def, clips);
    assert_ne!(variation_picks(&mut other, 24), picks, "each instance draws its own stream");
}

/// Shoulder at the origin; elbow and hand 10 apart along +X.
fn arm() -> Arc<Skeleton> {
    let mut s = Skeleton::default();
    s.push_local("shoulder", None, Xform::IDENTITY);
    s.push_local("elbow", Some(0), Xform::new(Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY));
    s.push_local("hand", Some(1), Xform::new(Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY));
    Arc::new(s)
}

#[test]
fn two_bone_ik_reaches_a_world_target_and_look_at_turns_the_chain() {
    let mut def = Definition::default();
    let target = param(&mut def, "hand_target", Value::Target(Target::default()));
    let yaw = param(&mut def, "yaw", Value::Float(0.0));
    let zero = def.add_value(ValueNode::Const(Value::Float(0.0)));
    let source = def.add_pose(PoseNode::Reference);
    let ik = def.add_pose(PoseNode::TwoBoneIk(Box::new(TwoBoneIk::new(source, "hand", target, true))));
    let bones = vec![("hand".to_owned(), 1.0)];
    let look = LookAt { source: ik, pitch: zero, yaw, weight: None, reference: String::new(), reference_rotation: Quat::IDENTITY, bones, yaw_limit: 45.0, pitch_limit: 45.0 };
    def.root = Some(def.add_pose(PoseNode::LookAt(Box::new(look))));
    let mut g = Instance::new(def.into_shared(), clips);
    let s = arm();
    let bind = Pose(s.bind_pose());
    assert_eq!(g.update(0.1, &bind), bind, "no rig, no target: passes through");
    g.rig = Some(Rig { skeleton: s.clone(), world: Xform::new(Vec3::new(100.0, 0.0, 0.0), Quat::IDENTITY) });
    assert_eq!(g.update(0.1, &bind), bind, "an unset target leaves the pose");
    let world = Xform::new(Vec3::new(108.0, 6.0, 0.0), Quat::from_rotation_z(0.5));
    g.params.set_by_name("hand_target", Value::Target(Target { set: true, bone: None, xform: world }));
    let hand = s.model_space(&g.update(0.1, &bind).0)[2];
    assert!(hand.pos.abs_diff_eq(Vec3::new(8.0, 6.0, 0.0), 1e-3), "{hand:?}");
    assert!(hand.rot.angle_between(world.rot) < 1e-3);
    g.params.set_by_name("yaw", Value::Float(90.0));
    let hand = s.model_space(&g.update(0.1, &bind).0)[2];
    assert!(hand.rot.angle_between(Quat::from_rotation_z(0.5 + 45f32.to_radians())) < 1e-3, "yaw clamps to its 45° limit");
}
