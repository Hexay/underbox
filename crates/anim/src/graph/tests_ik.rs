//! Two-bone IK node options: the enable blend over `blend_time`, both blend modes, and the
//! reference-pose twist.

use super::tests::clips;
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

/// Root at the origin, `mid` at `a`, `end` at `a + b` (bind pose, identity rotations).
fn chain(a: Vec3, b: Vec3) -> Arc<Skeleton> {
    let mut s = Skeleton::default();
    s.push_local("root", None, Xform::IDENTITY);
    s.push_local("mid", Some(0), Xform::new(a, Quat::IDENTITY));
    s.push_local("end", Some(1), Xform::new(b, Quat::IDENTITY));
    Arc::new(s)
}

/// An IK of `end` over `source` (default: the reference pose) toward param `t`, enabled by `on`.
fn ik_graph(edit: impl FnOnce(&mut TwoBoneIk), skeleton: &Arc<Skeleton>) -> Instance {
    let mut def = Definition::default();
    let on = param(&mut def, "on", Value::Bool(true));
    let target = param(&mut def, "t", Value::Target(Target::default()));
    let source = def.add_pose(PoseNode::Reference);
    let mut ik = TwoBoneIk { enabled: Some(on), ..TwoBoneIk::new(source, "end", target, false) };
    edit(&mut ik);
    def.root = Some(def.add_pose(PoseNode::TwoBoneIk(Box::new(ik))));
    let mut g = Instance::new(def.into_shared(), clips);
    g.rig = Some(Rig { skeleton: skeleton.clone(), world: Xform::IDENTITY });
    g
}

fn set_target(g: &mut Instance, pos: Vec3) {
    g.params.set_by_name("t", Value::Target(Target { set: true, bone: None, xform: Xform::new(pos, Quat::IDENTITY) }));
}

#[test]
fn enable_flips_blend_the_target_over_blend_time() {
    let s = chain(Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
    let bind = Pose(s.bind_pose());
    let mut g = ik_graph(|ik| ik.blend_time = 0.5, &s);
    set_target(&mut g, Vec3::new(10.0, 10.0, 0.0));
    let end = |g: &mut Instance| s.model_space(&g.update(0.1, &bind).0)[2].pos;
    assert!(end(&mut g).abs_diff_eq(Vec3::new(10.0, 10.0, 0.0), 1e-3), "enabled from the start: no blend in");
    g.params.set_by_name("on", Value::Bool(false));
    let p = end(&mut g);
    assert!(p.abs_diff_eq(Vec3::new(12.0, 8.0, 0.0), 1e-3), "0.1 s of 0.5: the goal is 80% of the way: {p:?}");
    (0..3).for_each(|_| _ = end(&mut g));
    assert!(end(&mut g).abs_diff_eq(Vec3::new(20.0, 0.0, 0.0), 1e-3), "fully out after 0.5 s");
    g.params.set_by_name("on", Value::Bool(true));
    assert!(end(&mut g).abs_diff_eq(Vec3::new(18.0, 2.0, 0.0), 1e-3), "and back in");
}

#[test]
fn pose_mode_blends_the_solved_rotations() {
    let s = chain(Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
    let bind = Pose(s.bind_pose());
    let mut g = ik_graph(|ik| (ik.blend_time, ik.blend_target) = (0.5, false), &s);
    set_target(&mut g, Vec3::new(10.0, 10.0, 0.0));
    // Solved, the root's turns cancel and the elbow bends 90°.
    let full = g.update(0.1, &bind).0[1].rot.angle_between(Quat::IDENTITY);
    g.params.set_by_name("on", Value::Bool(false));
    let part = g.update(0.1, &bind).0[1].rot.angle_between(Quat::IDENTITY);
    assert!((full - std::f32::consts::FRAC_PI_2).abs() < 1e-3 && (part - 0.8 * full).abs() < 1e-4, "the elbow turns 80% of its solved angle: {part} of {full}");
}

#[test]
fn chain_rotation_weight_twists_toward_the_reference_bend() {
    // Bind: bent in the XY plane, the end at (10, 10, 0). The input twists that plane 90° about
    // the root → end axis, so the elbow sits at (5, 5, -7.07) with the end unmoved.
    let s = chain(Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0));
    let bind = Pose(s.bind_pose());
    let axis = Vec3::new(1.0, 1.0, 0.0).normalize();
    let mut twisted = bind.0.clone();
    twisted[0].rot = Quat::from_axis_angle(axis, std::f32::consts::FRAC_PI_2);
    let solve = |twist: f32| {
        let mut def = Definition::default();
        let goal = Target { set: true, bone: None, xform: Xform::new(Vec3::new(10.0, 10.0, 0.0), Quat::IDENTITY) };
        let target = def.add_value(ValueNode::Const(Value::Target(goal)));
        let slot = def.slot("twisted");
        let source = def.add_pose(PoseNode::Clip { slot, looping: true, speed: 1.0 });
        def.root = Some(def.add_pose(PoseNode::TwoBoneIk(Box::new(TwoBoneIk { twist, ..TwoBoneIk::new(source, "end", target, false) }))));
        let input = twisted.clone();
        let mut g = Instance::new(def.into_shared(), move |_| {
            let mut c = crate::clip::Clip::new("twisted", 10.0);
            c.frames = vec![input.clone(); 2];
            Some(Arc::new(c))
        });
        g.rig = Some(Rig { skeleton: s.clone(), world: Xform::IDENTITY });
        s.model_space(&g.update(0.1, &bind).0)
    };
    let m = solve(0.0);
    assert!(m[1].pos.abs_diff_eq(Vec3::new(5.0, 5.0, -50f32.sqrt()), 1e-3), "no twist keeps the input's plane: {:?}", m[1].pos);
    let m = solve(1.0);
    assert!(m[1].pos.abs_diff_eq(Vec3::new(10.0, 0.0, 0.0), 1e-3), "full twist restores the reference bend: {:?}", m[1].pos);
    assert!(m[2].pos.abs_diff_eq(Vec3::new(10.0, 10.0, 0.0), 1e-3) && m[2].rot.angle_between(Quat::IDENTITY) < 1e-3, "the end keeps the target");
    let half = solve(0.5)[1].pos - Vec3::new(5.0, 5.0, 0.0);
    assert!((half.angle_between(Vec3::new(5.0, -5.0, 0.0)).to_degrees() - 45.0).abs() < 1e-2, "half way round: {half:?}");
}
