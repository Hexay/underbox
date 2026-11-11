use super::*;
use crate::clip::Clip;
use crate::graph::{Instance, PoseNode};
use crate::pose::Pose;
use crate::xform::Xform;
use glam::Quat;
use std::sync::Arc;

/// Grounded (1D blend idle → walk over `speed`) ⇄ Jump (one-shot clip), in `.ag` JSON as the
/// editor writes it (trimmed of layout fields).
const GRAPH: &str = r#"{
  "Version": 1, "Model": "models/test.vmdl",
  "parameters": [
    { "Type": "Float", "Name": "speed", "RuntimeDefaultFloat": 0, "Min": 0, "Max": 100 },
    { "Type": "Bool", "Name": "b_jump", "RuntimeDefaultBool": false },
    { "Type": "ID", "Name": "holdtype", "RuntimeDefaultID": "none", "ExpectedValues": ["none", "pistol"] },
    { "Type": "Target", "Name": "ik.hand" }
  ],
  "nodes": [
    { "_class": "PoseResultNode", "Pose": { "Identifier": "sm", "Output": "Result" }, "Identifier": "out" },
    { "_class": "StateMachineNode", "Identifier": "sm", "DefaultStateIndex": 0,
      "States": [
        { "Name": "Grounded", "Type": "BlendTree", "Graph": { "nodes": [
          { "_class": "PoseResultNode", "Pose": { "Identifier": "blend", "Output": "Result" }, "Identifier": "g_out" },
          { "_class": "FloatParameterNode", "ParameterName": "speed", "Identifier": "p_speed" },
          { "_class": "Blend1DNode", "Identifier": "blend", "Parameter": { "Identifier": "p_speed", "Output": "Value" }, "Points": [
            { "Value": 100, "Graph": { "nodes": [
              { "_class": "PoseResultNode", "Pose": { "Identifier": "walk", "Output": "Result" }, "Identifier": "w_out" },
              { "_class": "AnimationClipNode", "Identifier": "walk", "Loop": true, "DefaultVariationData": { "AnimationName": "clips/walk.anim", "SpeedMultiplier": 1 } } ] } },
            { "Value": 0, "Graph": { "nodes": [
              { "_class": "PoseResultNode", "Pose": { "Identifier": "idle", "Output": "Result" }, "Identifier": "i_out" },
              { "_class": "AnimationClipNode", "Identifier": "idle", "Loop": true, "DefaultVariationData": { "AnimationName": "clips/idle.anim" } } ] } } ] } ] } },
        { "Name": "Jump", "Type": "BlendTree",
          "Events": [ { "Id": "takeoff" }, { "Id": "landing", "IsEntry": false, "IsExit": true }, { "Id": " " } ],
          "TimedEvents": [ { "Id": "airborne", "Type": "TimeElapsed", "Operator": "GreaterThanEqual", "Time": 0.1 }, { "Id": "late", "Type": "TimeRemaining", "Operator": "LessThanEqual", "Time": 0.25 } ],
          "Graph": { "nodes": [
          { "_class": "PoseResultNode", "Pose": { "Identifier": "jump", "Output": "Result" }, "Identifier": "j_out" },
          { "_class": "AnimationClipNode", "Identifier": "jump", "Loop": false, "DefaultVariationData": { "AnimationName": "clips/jump.anim" } } ] } }
      ],
      "Conduits": [
        { "From": "Grounded", "To": "Jump", "Graph": { "nodes": [
          { "_class": "BoolParameterNode", "ParameterName": "b_jump", "Identifier": "p_jump" },
          { "_class": "TransitionNode", "Condition": { "Identifier": "p_jump", "Output": "Value" }, "Duration": 0, "Identifier": "t1" } ] } },
        { "From": "Jump", "To": "Grounded", "Graph": { "nodes": [
          { "_class": "StateCompletedConditionNode", "Identifier": "done" },
          { "_class": "TransitionNode", "Condition": { "Identifier": "done", "Output": "Value" }, "Duration": 0, "Identifier": "t2" } ] } }
      ],
      "GlobalGraph": { "nodes": [ { "_class": "GlobalTransitionNode", "StateName": "Grounded", "Condition": { "Identifier": null, "Output": null } } ] }
    },
    { "_class": "Blend2DNode", "Identifier": "unused" }
  ]
}"#;

fn hold(name: &str, x: f32) -> Arc<Clip> {
    let mut c = Clip::new(name, 10.0);
    c.frames = vec![vec![Xform::new(glam::Vec3::new(x, 0.0, 0.0), Quat::IDENTITY)]; 6];
    Arc::new(c)
}

#[test]
fn compiles_and_runs_a_state_machine_graph() {
    let c = compile(GRAPH).unwrap();
    assert!(c.unsupported.is_empty(), "unreferenced nodes aren't compiled: {:?}", c.unsupported);
    let def = c.def;
    assert_eq!(def.model, "models/test.vmdl");
    assert_eq!(def.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["speed", "b_jump", "holdtype", "ik.hand"]);
    assert_eq!(def.params[2].default, Value::Id("none".into()));
    assert_eq!(def.slots, ["clips/walk.anim", "clips/idle.anim", "clips/jump.anim"]);
    let Some(PoseNode::StateMachine { states, .. }) = def.root.map(|r| &def.poses[r]) else { panic!("root is the state machine") };
    assert_eq!(states.iter().map(|s| s.transitions.len()).collect::<Vec<_>>(), [1, 1], "a global transition without a condition never fires");

    let clip = |n: &str| Some(hold(n, if n.contains("walk") { 10.0 } else if n.contains("jump") { 50.0 } else { 0.0 }));
    let mut g = Instance::new(def.into_shared(), clip);
    let bind = Pose::zero(1);
    g.params.set_by_name("speed", Value::Float(50.0));
    assert!((g.update(0.1, &bind).0[0].pos.x - 5.0).abs() < 1e-4);
    g.params.set_by_name("b_jump", Value::Bool(true));
    assert_eq!(g.update(0.1, &bind).0[0].pos.x, 50.0, "zero-duration transition cuts straight to Jump");
    assert_eq!(g.active_tags(), ["takeoff"], "entry events default on; blank IDs skipped; 0.5 s left");
    g.update(0.1, &bind);
    assert_eq!(g.active_tags(), ["airborne"], "0.1 s into Jump");
    g.update(0.1, &bind);
    assert_eq!(g.active_tags(), ["airborne"], "0.3 s left");
    g.update(0.1, &bind);
    assert_eq!(g.active_tags(), ["airborne", "late"], "0.2 s left");
    g.params.set_by_name("b_jump", Value::Bool(false));
    let back = (0..10).position(|_| g.update(0.1, &bind).0[0].pos.x != 50.0);
    assert_eq!(back, Some(1), "the 0.5 s jump completes, then Grounded resumes");
    assert_eq!(g.active_tags(), ["landing"], "Jump's exit event on the update that leaves it");
}

#[test]
fn reports_unsupported_nodes() {
    let src = r#"{ "parameters": [], "nodes": [
        { "_class": "PoseResultNode", "Pose": { "Identifier": "x" }, "Identifier": "out" },
        { "_class": "LayerBlendNode", "Identifier": "x", "Base": { "Identifier": "z" } },
        { "_class": "ZeroPoseNode", "Identifier": "z" } ] }"#;
    let c = compile(src).unwrap();
    assert_eq!(c.unsupported, ["LayerBlendNode"]);
    assert_eq!(c.def.root.map(|r| &c.def.poses[r]), Some(&PoseNode::Zero), "layer blends pass their base through");
}

/// The citizen graph from sbox-public, when it's checked out next to us.
#[test]
fn compiles_citizen_human_graph() {
    let path = "/srv/agents/work/sbox-public/game/addons/citizen/Assets/models/citizen_human/animgraphs/human.ag";
    let Ok(src) = std::fs::read_to_string(path) else { return };
    let c = compile(&src).unwrap();
    assert!(c.def.params.len() > 50 && c.def.param("move_x").is_some() && c.def.param("holdtype").is_some());
    assert!(c.def.root.is_some() && !c.def.slots.is_empty(), "root {:?}, {} poses, unsupported {:?}", c.def.root, c.def.poses.len(), c.unsupported);
    let real = ["Blend2DNode", "LookAtNode", "TwoBoneIKNode", "FloatSpringNode", "FloatEaseNode", "SelectorNode", "ParameterizedSelectorNode", "ConstBoneTargetNode"];
    assert!(real.iter().all(|k| !c.unsupported.iter().any(|u| u == k)), "{:?}", c.unsupported);
    let count = |f: fn(&PoseNode) -> bool| c.def.poses.iter().filter(|p| f(p)).count();
    assert!(count(|p| matches!(p, PoseNode::Blend2D { triangles, .. } if !triangles.is_empty())) > 0);
    assert!(count(|p| matches!(p, PoseNode::LookAt(_))) > 0 && count(|p| matches!(p, PoseNode::TwoBoneIk(_))) > 0);
    let lookat = c.def.poses.iter().find_map(|p| if let PoseNode::LookAt(l) = p { Some(l) } else { None }).unwrap();
    assert!(!lookat.reference.is_empty() && lookat.bones.windows(2).all(|w| w[0].1 >= w[1].1), "{lookat:?}");
}
