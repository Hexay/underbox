use super::*;
use crate::clip::Clip;
use crate::pose::Pose;
use crate::xform::Xform;
use glam::{Quat, Vec3};

/// A one-bone clip holding bone 0 at x = `x` for 1 second (11 frames at 10 fps).
fn hold(name: &str, x: f32) -> Arc<Clip> {
    let mut c = Clip::new(name, 10.0);
    c.frames = vec![vec![Xform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY)]; 11];
    Arc::new(c)
}

pub(super) fn clips(name: &str) -> Option<Arc<Clip>> {
    match name {
        "idle" => Some(hold(name, 0.0)),
        "walk" => Some(hold(name, 10.0)),
        "run" => Some(hold(name, 30.0)),
        "jump" => Some(hold(name, 100.0)),
        _ => None,
    }
}

pub(super) fn x(pose: &Pose) -> f32 {
    pose.0[0].pos.x
}

pub(super) fn clip(def: &mut Definition, name: &str, looping: bool) -> PoseId {
    let slot = def.slot(name);
    def.add_pose(PoseNode::Clip { slot, looping, speed: 1.0 })
}

/// speed blends idle (0) → walk (100) → run (300).
fn locomotion(def: &mut Definition) -> PoseId {
    def.params.push(ParamDef::new("speed", Value::Float(0.0)));
    let speed = def.add_value(ValueNode::Param(0));
    let points = vec![(0.0, clip(def, "idle", true)), (100.0, clip(def, "walk", true)), (300.0, clip(def, "run", true))];
    def.add_pose(PoseNode::Blend1D { param: speed, points })
}

#[test]
fn blend1d_follows_parameter() {
    let mut def = Definition::default();
    def.root = Some(locomotion(&mut def));
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    assert_eq!(x(&g.update(0.1, &bind)), 0.0);
    g.params.set_by_name("speed", Value::Float(50.0));
    assert!((x(&g.update(0.1, &bind)) - 5.0).abs() < 1e-4);
    g.params.set_by_name("speed", Value::Float(200.0));
    assert!((x(&g.update(0.1, &bind)) - 20.0).abs() < 1e-4);
    g.params.set_by_name("speed", Value::Float(999.0));
    assert!((x(&g.update(0.1, &bind)) - 30.0).abs() < 1e-4, "clamps past the last point");
}

/// Grounded (locomotion) → Jump on b_jump, blending 0.2 s; Jump → Grounded once the
/// non-looping jump clip completes.
fn jump_machine() -> (Definition, PoseId) {
    let mut def = Definition::default();
    let ground = locomotion(&mut def);
    def.params.push(ParamDef::new("b_jump", Value::Bool(false)));
    let b_jump = def.add_value(ValueNode::Param(1));
    let completed = def.add_value(ValueNode::StateCompleted);
    let jump = clip(&mut def, "jump", false);
    let states = vec![
        State { name: "Grounded".into(), pose: ground, transitions: vec![Transition { to: 1, condition: b_jump, duration: 0.2 }] },
        State { name: "Jump".into(), pose: jump, transitions: vec![Transition { to: 0, condition: completed, duration: 0.0 }] },
    ];
    let sm = def.add_pose(PoseNode::StateMachine { states, default: 0 });
    def.root = Some(sm);
    (def, sm)
}

#[test]
fn state_machine_transitions_and_blends() {
    let (def, sm) = jump_machine();
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    g.update(0.1, &bind);
    assert_eq!(g.current_state(sm), Some("Grounded"));
    g.params.set_by_name("b_jump", Value::Bool(true));
    assert_eq!(x(&g.update(0.1, &bind)), 0.0, "the new state enters at weight 0");
    assert_eq!(g.current_state(sm), Some("Jump"));
    let mid = x(&g.update(0.1, &bind));
    assert!((mid - 50.0).abs() < 1e-3, "halfway through the 0.2 s blend from 0 to 100: {mid}");
    assert!((x(&g.update(0.1, &bind)) - 100.0).abs() < 1e-3, "blend done");
    g.params.set_by_name("b_jump", Value::Bool(false));
    // The 1 s jump clip is at 0.2 s, so it completes on the 8th update (7 more in Jump).
    let updates = (0..20).take_while(|_| {
        g.update(0.1, &bind);
        g.current_state(sm) == Some("Jump")
    });
    let n = updates.count();
    assert!((7..=8).contains(&n), "left Jump {n} updates later");
    assert_eq!(g.current_state(sm), Some("Grounded"), "completion takes the exit transition");
}

#[test]
fn time_in_state_and_value_math() {
    let mut def = Definition::default();
    let a = clip(&mut def, "idle", true);
    let b = clip(&mut def, "walk", true);
    let after = def.add_value(ValueNode::TimeInState { cmp: Cmp::Ge, seconds: 0.25 });
    let states = vec![
        State { name: "A".into(), pose: a, transitions: vec![Transition { to: 1, condition: after, duration: 0.0 }] },
        State { name: "B".into(), pose: b, transitions: vec![] },
    ];
    let sm = def.add_pose(PoseNode::StateMachine { states, default: 0 });
    def.root = Some(sm);
    def.params.push(ParamDef::new("v", Value::Float(4.0)));
    def.params.push(ParamDef::new("state", Value::Id("swim".into())));
    let v = def.add_value(ValueNode::Param(0));
    let id = def.add_value(ValueNode::Param(1));
    let sum = def.add_value(ValueNode::FloatMath { a: v, b: Operand::Const(-6.0), op: MathOp::Add, abs: true, negate: false });
    let remap = def.add_value(ValueNode::FloatRemap { value: v, from: (0.0, 8.0), to: (10.0, 20.0) });
    let is_swim = def.add_value(ValueNode::IdMatches { id, ids: vec!["SWIM".into()], negate: false });
    let shared = def.into_shared();
    let mut g = Instance::new(shared.clone(), clips);
    let values = value::Values { def: &shared, params: &g.params, ctx: Default::default(), smooth: &[] };
    assert_eq!((values.float(sum), values.float(remap), values.bool(is_swim)), (2.0, 15.0, true));
    let unary = |op| value::math(-2.75, 0.0, op);
    assert_eq!(
        [MathOp::Floor, MathOp::Ceil, MathOp::Trunc, MathOp::Fract, MathOp::InvFract].map(unary),
        [-3.0, -2.0, -2.0, -0.75, 1.75]
    );
    assert_eq!((value::math(-7.0, 3.0, MathOp::Mod), value::math(7.0, 0.0, MathOp::Mod)), (-1.0, 0.0));
    let bind = Pose::zero(1);
    g.update(0.2, &bind);
    assert_eq!(g.current_state(sm), Some("A"));
    g.update(0.1, &bind);
    assert_eq!(g.current_state(sm), Some("B"));
}

#[test]
fn recording_replays_updates_with_their_parameters() {
    use recording::{Seek, UpdateKind};
    let mut def = Definition::default();
    def.root = Some(locomotion(&mut def));
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    let mut rec = Recording::default();
    assert!(!rec.start_playback(), "nothing recorded yet");
    rec.start();
    let mut live = vec![];
    for speed in [0.0, 50.0, 200.0] {
        g.params.set_by_name("speed", Value::Float(speed));
        live.push(x(&g.update(0.1, &bind)));
        rec.record(0.1, &g.params, Xform::IDENTITY);
    }
    rec.stop();
    assert_eq!((rec.len(), rec.get(0).map(|u| u.kind), rec.get(1).map(|u| u.kind)), (3, Some(UpdateKind::FullState), Some(UpdateKind::TimeStep)));
    assert_eq!(rec.seek(1, &mut g, &bind).err(), Some(Seek::Failure), "seeking needs playback");
    assert!(rec.start_playback());
    for (i, want) in live.iter().enumerate() {
        assert!((x(&rec.seek(i, &mut g, &bind).unwrap()) - want).abs() < 1e-4);
        assert_eq!(rec.current_index(), Some(i));
    }
    assert_eq!(rec.seek(3, &mut g, &bind).err(), Some(Seek::Failure));
    rec.clear();
    assert!(rec.has_data(), "can't clear during playback");
    rec.stop_playback();
    rec.clear();
    assert!(!rec.has_data());
}
