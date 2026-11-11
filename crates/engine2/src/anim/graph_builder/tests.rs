//! Graphs built node by node through the builder's slot functions, as managed's GraphBuilder
//! does, then converted (directly, and through a Compile → CreateCompiled round trip).

use super::items::{self, list, push_int, push_item};
use super::{add_node, capture, create, delete, intern, snapshot, with};
use crate::anim::graph::built;
use abi::{NativeRef, class};
use anim::graph::{Definition, Operand, PoseNode, Transition, ValueNode};
use anim::params::Value;
use snapshot::ParamDesc;

type B = NativeRef<class::CGraphBuilder>;

macro_rules! field {
    ($ty:ty, $m:ident :: $g:ident, $obj:expr, $v:expr) => {
        abi::rt::fields::set::<$ty, { abi::slots::$m::$g.index }>($obj.as_raw(), $v)
    };
}

fn convert(b: B, root: i32) -> Definition {
    super::convert::convert(&capture(b, root, "models/citizen.vmdl", "").unwrap()).def
}

fn float_param(b: B, name: &str, min: f32, max: f32) {
    let p = add_node::<class::CControlParameterFloatNode_Definition>(b);
    field!(f32, c_control_parameter_float_node_definition::GET_M_DEFAULT_VALUE, p, 0.0);
    let id = intern(b, name);
    with(b, |s| {
        s.meta.control_params.push(id);
        s.meta.params.push(ParamDesc { name: name.into(), kind: 3, min, max, options: vec![] });
    });
}

fn clip(b: B, slot: i16) -> NativeRef<class::CClipNode_Definition> {
    let c = add_node::<class::CClipNode_Definition>(b);
    field!(i16, c_clip_node_definition::GET_M_DATA_SLOT_IDX, c, slot);
    field!(bool, c_clip_node_definition::GET_M_ALLOW_LOOPING, c, true);
    field!(f32, c_clip_node_definition::GET_M_SPEED_MULTIPLIER, c, 1.0);
    c
}

fn animations(b: B, names: &[&str]) {
    with(b, |s| s.meta.animations = names.iter().map(|n| n.to_string()).collect());
}

/// A float parameter (node 0) drives a blend1d (node 3) of idle (1) and walk (2) over 0..5.
fn locomotion() -> B {
    let b = create();
    float_param(b, "speed", 0.0, 10.0);
    clip(b, 0);
    clip(b, 1);
    let blend = add_node::<class::CBlend1DNode_Definition>(b);
    let sources = list::<_, class::CUtlVector_int16_>(blend, "SourceNodeIndices");
    push_int(sources, 1);
    push_int(sources, 2);
    let range = push_item::<_, class::CParameterizedBlendNode_BlendRange>(list::<_, class::CUtlVector_CParameterizedBlendNode_BlendRange_>(blend, "BlendRanges"));
    field!(i16, c_parameterized_blend_node_blend_range::GET_M_INPUT_IDX0, range, 0);
    field!(i16, c_parameterized_blend_node_blend_range::GET_M_INPUT_IDX1, range, 1);
    field!(f32, c_parameterized_blend_node_blend_range::GET_M_PARAM_RANGE_START, range, 0.0);
    field!(f32, c_parameterized_blend_node_blend_range::GET_M_PARAM_RANGE_END, range, 5.0);
    field!(i16, c_blend1_d_node_definition::GET_M_PARAMETER_VALUE_NODE_IDX, blend, 0);
    animations(b, &["idle", "walk"]);
    b
}

#[test]
fn float_parameter_drives_blend1d() {
    let b = locomotion();
    let def = convert(b, 3);
    assert_eq!(def.slots, ["idle", "walk"]);
    assert_eq!(def.model, "models/citizen.vmdl");
    let p = &def.params[0];
    assert_eq!((p.name.as_str(), &p.default, p.min, p.max), ("speed", &Value::Float(0.0), 0.0, 10.0));
    let Some(PoseNode::Blend1D { param, points }) = def.root.map(|r| &def.poses[r]) else { panic!("{def:?}") };
    assert_eq!(def.values[*param], ValueNode::Param(0));
    let clips: Vec<_> = points.iter().map(|&(x, p)| (x, def.poses[p].clone())).collect();
    let clip = |slot| PoseNode::Clip { slot, looping: true, speed: 1.0 };
    assert_eq!(clips, [(0.0, clip(0)), (5.0, clip(1))]);
    delete(b);
}

/// Bool `jump` (node 0); states over idle (1) and jump (2); A → B on `jump` over 0.25 s.
fn jump_machine() -> B {
    let b = create();
    let p = add_node::<class::CControlParameterBoolNode_Definition>(b);
    field!(bool, c_control_parameter_bool_node_definition::GET_M_DEFAULT_VALUE, p, false);
    let id = intern(b, "jump");
    with(b, |s| s.meta.control_params.push(id));
    clip(b, 0);
    clip(b, 1);
    for child in [1, 2] {
        let s = add_node::<class::CStateNode_Definition>(b);
        field!(i16, c_state_node_definition::GET_M_CHILD_NODE_IDX, s, child);
    }
    let t = add_node::<class::CTransitionNode_Definition>(b);
    field!(i16, c_transition_node_definition::GET_M_TARGET_STATE_NODE_IDX, t, 4);
    field!(f32, c_transition_node_definition::GET_M_DURATION, t, 0.25);
    let sm = add_node::<class::CStateMachineNode_Definition>(b);
    let states = list::<_, class::CUtlVector_CStateMachineNode_StateDefinition_>(sm, "StateDefinition");
    for (k, node) in [3, 4].into_iter().enumerate() {
        let st = push_item::<_, class::CStateMachineNode_StateDefinition>(states);
        field!(i16, c_state_machine_node_state_definition::GET_M_STATE_NODE_IDX, st, node);
        field!(i16, c_state_machine_node_state_definition::GET_M_ENTRY_CONDITION_NODE_IDX, st, -1);
        let routes = list::<_, class::CUtlVector_CStateMachineNode_TransitionDefinition_>(st, "TransitionDefinition");
        if k == 0 {
            let r = push_item::<_, class::CStateMachineNode_TransitionDefinition>(routes);
            field!(i16, c_state_machine_node_transition_definition::GET_M_TARGET_STATE_IDX, r, 1);
            field!(i16, c_state_machine_node_transition_definition::GET_M_TRANSITION_NODE_IDX, r, 5);
            field!(i16, c_state_machine_node_transition_definition::GET_M_CONDITION_NODE_IDX, r, 0);
        }
    }
    field!(i16, c_state_machine_node_definition::GET_M_DEFAULT_STATE_INDEX, sm, 0);
    animations(b, &["idle", "jump"]);
    b
}

#[test]
fn state_machine_transitions_on_bool() {
    let b = jump_machine();
    let def = convert(b, 6);
    delete(b);
    assert_eq!((def.params[0].name.as_str(), &def.params[0].default), ("jump", &Value::Bool(false)));
    let Some(PoseNode::StateMachine { states, default }) = def.root.map(|r| &def.poses[r]) else { panic!("{def:?}") };
    assert_eq!((*default, states.len()), (0, 2));
    let slot = |s: usize| match def.poses[states[s].pose] {
        PoseNode::Clip { slot, .. } => slot,
        ref p => panic!("{p:?}"),
    };
    assert_eq!((slot(0), slot(1)), (0, 1));
    let [Transition { to: 1, condition, duration }] = states[0].transitions[..] else { panic!("{:?}", states[0].transitions) };
    assert_eq!((def.values[condition].clone(), duration), (ValueNode::Param(0), 0.25));
    assert!(states[1].transitions.is_empty());
}

#[test]
fn compile_round_trips_through_create_compiled() {
    let b = jump_machine();
    let snap = capture(b, 6, "m.vmdl", "v1").unwrap();
    delete(b);
    let bytes = snapshot::encode(&snap);
    let back = snapshot::decode(&bytes).expect("our snapshot");
    assert_eq!(back, snap);
    assert_eq!(super::convert::convert(&back).def, super::convert::convert(&snap).def);
    let g = built(super::definition("runtime_animgraph", &back));
    let def = crate::anim::graph::definition(g).unwrap();
    assert_eq!((def.model.as_str(), def.variation.as_str(), def.slots.len()), ("m.vmdl", "v1", 2));
    assert!(snapshot::decode(b"\x01\x02facepunch").is_none(), "foreign bytes are no snapshot");
}

/// ID `move` (0) compared to `Run` (1) picks a speed (2) scaling (4) a scale node (5) over the
/// reference pose (3); a spring (6) nothing reads.
#[test]
fn id_comparison_keeps_names_and_stand_ins_are_listed() {
    let b = create();
    let p = add_node::<class::CControlParameterIDNode_Definition>(b);
    items::set(p, "DefaultValue", intern(b, "Walk") as i64);
    let id = intern(b, "move");
    with(b, |s| {
        s.meta.control_params.push(id);
        s.meta.params.push(ParamDesc { name: "move".into(), kind: 2, min: 0.0, max: 0.0, options: vec!["Walk".into(), "Run".into()] });
    });
    let cmp = add_node::<class::CIDComparisonNode_Definition>(b);
    field!(i16, cid_comparison_node_definition::GET_M_INPUT_VALUE_NODE_IDX, cmp, 0);
    let run = intern(b, "Run");
    assert_eq!(intern(b, "run"), run, "IDs are case-insensitive; the first spelling names them");
    push_int(list::<_, class::CUtlVector_StringID_>(cmp, "ComparisionIDs"), run as i64);
    let switch = add_node::<class::CFloatSwitchNode_Definition>(b);
    field!(i16, c_float_switch_node_definition::GET_M_SWITCH_VALUE_NODE_IDX, switch, 1);
    field!(i16, c_float_switch_node_definition::GET_M_TRUE_VALUE_NODE_IDX, switch, -1);
    field!(i16, c_float_switch_node_definition::GET_M_FALSE_VALUE_NODE_IDX, switch, -1);
    field!(f32, c_float_switch_node_definition::GET_M_TRUE_VALUE, switch, 2.0);
    field!(f32, c_float_switch_node_definition::GET_M_FALSE_VALUE, switch, 1.0);
    add_node::<class::CReferencePoseNode_Definition>(b);
    let scale = add_node::<class::CSpeedScaleNode_Definition>(b);
    field!(i16, c_speed_scale_node_definition::GET_M_CHILD_NODE_IDX, scale, 5);
    field!(i16, c_speed_scale_node_definition::GET_M_INPUT_VALUE_NODE_IDX, scale, 2);
    let scale_node = add_node::<class::CScaleNode_Definition>(b);
    field!(i16, c_scale_node_definition::GET_M_CHILD_NODE_IDX, scale_node, 3);
    add_node::<class::CFloatSpringNode_Definition>(b);
    let snap = capture(b, 4, "", "").unwrap();
    delete(b);
    assert!(items::item(scale_node).is_none(), "nodes die with their builder");

    let out = super::convert::convert(&snap);
    let def = &out.def;
    let p = &def.params[0];
    assert_eq!((&p.default, &p.options[..]), (&Value::Id("Walk".into()), &["Walk".to_string(), "Run".to_string()][..]));
    let Some(&PoseNode::SpeedScale { source, speed }) = def.root.map(|r| &def.poses[r]) else { panic!("{def:?}") };
    assert_eq!(def.poses[source], PoseNode::Reference, "the scale node passes its child through");
    let ValueNode::Select { cond, yes: Operand::Const(2.0), no: Operand::Const(1.0) } = def.values[speed] else { panic!("{:?}", def.values[speed]) };
    let ValueNode::IdMatches { id, ref ids, negate: false } = def.values[cond] else { panic!("{:?}", def.values[cond]) };
    assert_eq!((&def.values[id], &ids[..]), (&ValueNode::Param(0), &["Run".to_string()][..]));
    assert_eq!(out.unsupported, ["CScaleNode"]);
}
