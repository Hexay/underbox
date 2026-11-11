//! Builder graphs using blend2D, ID selectors, eases and two-bone IK, converted and evaluated.

use super::items::{self, list, push_int, push_vec2};
use super::{add_node, capture, create, delete, intern, snapshot::ParamDesc, with};
use abi::{NativeRef, class};
use anim::graph::{Easing, Instance, PoseNode, Rig, ValueNode};
use anim::params::{Target, Value};
use anim::{Pose, Skeleton, Xform};
use glam::{Quat, Vec3};
use std::sync::Arc;

type B = NativeRef<class::CGraphBuilder>;

macro_rules! field {
    ($ty:ty, $m:ident :: $g:ident, $obj:expr, $v:expr) => {
        abi::rt::fields::set::<$ty, { abi::slots::$m::$g.index }>($obj.as_raw(), $v)
    };
}

fn param<C: abi::NativeClass>(b: B, name: &str, kind: i64, options: &[&str]) -> NativeRef<C> {
    let p = add_node::<C>(b);
    let id = intern(b, name);
    with(b, |s| {
        s.meta.control_params.push(id);
        s.meta.params.push(ParamDesc { name: name.into(), kind, min: 0.0, max: 0.0, options: options.iter().map(|o| o.to_string()).collect() });
    });
    p
}

fn clip(b: B, slot: i16) {
    let c = add_node::<class::CClipNode_Definition>(b);
    field!(i16, c_clip_node_definition::GET_M_DATA_SLOT_IDX, c, slot);
    field!(f32, c_clip_node_definition::GET_M_SPEED_MULTIPLIER, c, 1.0);
}

/// x (0), y (1), move (2); clips 3..=5; blend2d (6) over a right triangle, its x eased (8); an
/// ID selector (7) of `Run` → clip 4, else clip 3.
#[test]
fn blend2d_id_selector_and_ease_convert() {
    let b = create();
    param::<class::CControlParameterFloatNode_Definition>(b, "x", 3, &[]);
    param::<class::CControlParameterFloatNode_Definition>(b, "y", 3, &[]);
    param::<class::CControlParameterIDNode_Definition>(b, "move", 2, &["Walk", "Run"]);
    (0..3).for_each(|s| clip(b, s));
    let blend = add_node::<class::CBlend2DNode_Definition>(b);
    let sources = list::<_, class::CUtlVector_int16_>(blend, "SourceNodeIndices");
    let values = list::<_, class::CUtlVector_Vector2D_>(blend, "Values");
    for (src, at) in [(3, [0.0, 0.0]), (4, [1.0, 0.0]), (5, [0.0, 1.0])] {
        push_int(sources, src);
        push_vec2(values, at);
    }
    let tris = list::<_, class::CUtlVector_int16_>(blend, "Indices");
    [0, 1, 2].into_iter().for_each(|i| push_int(tris, i));
    field!(i16, c_blend2_d_node_definition::GET_M_INPUT_PARAMETER_NODE_IDX0, blend, 8);
    field!(i16, c_blend2_d_node_definition::GET_M_INPUT_PARAMETER_NODE_IDX1, blend, 1);
    let sel = add_node::<class::CIDBasedSelectorNode_Definition>(b);
    push_int(list::<_, class::CUtlVector_StringID_>(sel, "OptionIDs"), intern(b, "Run") as i64);
    push_int(list::<_, class::CUtlVector_int16_>(sel, "OptionNodeIndices"), 4);
    field!(i16, cid_based_selector_node_definition::GET_M_PARAMETER_NODE_IDX, sel, 2);
    field!(i16, cid_based_selector_node_definition::GET_M_FALLBACK_NODE_IDX, sel, 3);
    let ease = add_node::<class::CFloatEaseNode_Definition>(b);
    field!(i16, c_float_ease_node_definition::GET_M_INPUT_VALUE_NODE_IDX, ease, 0);
    field!(f32, c_float_ease_node_definition::GET_M_EASE_TIME, ease, 0.5);
    items::set(ease, "EasingOp", 3);
    let snap = capture(b, 6, "", "").unwrap();
    delete(b);

    let out = super::convert::convert(&snap);
    let def = &out.def;
    let Some(PoseNode::Blend2D { x, y, points, triangles }) = def.root.map(|r| &def.poses[r]) else { panic!("{def:?}") };
    let ValueNode::Ease { input, time: 0.5, easing: Easing::InOutQuad, start: None } = def.values[*x] else { panic!("{:?}", def.values[*x]) };
    assert_eq!((&def.values[input], &def.values[*y]), (&ValueNode::Param(0), &ValueNode::Param(1)));
    assert_eq!((points.iter().map(|p| p.0).collect::<Vec<_>>(), &triangles[..]), (vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], &[[0, 1, 2]][..]));
    assert!(out.unsupported.is_empty(), "{:?}", out.unsupported);

    let def = super::convert::convert(&super::snapshot::Snapshot { root: 7, ..snap }).def;
    let Some(PoseNode::Selector { cases, fallback }) = def.root.map(|r| &def.poses[r]) else { panic!("{def:?}") };
    let [(cond, run)] = cases[..] else { panic!("{cases:?}") };
    let ValueNode::IdMatches { id, ref ids, negate: false } = def.values[cond] else { panic!("{:?}", def.values[cond]) };
    assert_eq!((&def.values[id], &ids[..]), (&ValueNode::Param(2), &["Run".to_string()][..]));
    assert!(matches!((&def.poses[run], &def.poses[*fallback]), (PoseNode::Clip { slot: 1, .. }, PoseNode::Clip { slot: 0, .. })));
}

/// A target parameter (0) drives a two-bone IK (2) of `hand` over the reference pose (1).
#[test]
fn two_bone_ik_from_the_builder_reaches_its_target() {
    let b = create();
    param::<class::CControlParameterTargetNode_Definition>(b, "ik.hand", 5, &[]);
    add_node::<class::CReferencePoseNode_Definition>(b);
    let ik = add_node::<class::CTwoBoneIKNode_Definition>(b);
    field!(i16, c_two_bone_ik_node_definition::GET_M_CHILD_NODE_IDX, ik, 1);
    field!(i16, c_two_bone_ik_node_definition::GET_M_EFFECTOR_TARGET_NODE_IDX, ik, 0);
    field!(bool, c_two_bone_ik_node_definition::GET_M_IS_TARGET_IN_WORLD_SPACE, ik, true);
    items::set(ik, "EffectorBoneID", intern(b, "hand") as i64);
    let snap = capture(b, 2, "", "").unwrap();
    delete(b);
    let out = super::convert::convert(&snap);
    assert!(out.unsupported.is_empty(), "{:?}", out.unsupported);

    let mut s = Skeleton::default();
    s.push_local("shoulder", None, Xform::IDENTITY);
    s.push_local("elbow", Some(0), Xform::new(Vec3::new(0.0, 0.0, -10.0), Quat::IDENTITY));
    s.push_local("hand", Some(1), Xform::new(Vec3::new(0.0, 0.0, -10.0), Quat::IDENTITY));
    let s = Arc::new(s);
    let mut g = Instance::new(out.def.into_shared(), |_| None);
    g.rig = Some(Rig { skeleton: s.clone(), world: Xform::new(Vec3::new(0.0, 0.0, 50.0), Quat::IDENTITY) });
    let target = Xform::new(Vec3::new(12.0, 0.0, 45.0), Quat::IDENTITY);
    g.params.set_by_name("ik.hand", Value::Target(Target { set: true, bone: None, xform: target }));
    let hand = s.model_space(&g.update(0.1, &Pose(s.bind_pose())).0)[2];
    assert!(hand.pos.abs_diff_eq(Vec3::new(12.0, 0.0, -5.0), 1e-3), "world target, character at z 50: {hand:?}");
}
