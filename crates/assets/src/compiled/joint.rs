//! Physics joints (`VPhysXJoint_t`, an aggregate's `m_joints`): ragdoll constraints between parts.
//! Frames are transform arrays: pos, scale, quat (8 floats) or pos, quat (7).

use super::kv::{self, float, int, int_value, list, obj, objects};
use ::kv::{Object, Value};

pub const SPHERICAL: i64 = 1;
pub const PRISMATIC: i64 = 2;
pub const REVOLUTE: i64 = 3;
pub const CONICAL: i64 = 4;
pub const WELD: i64 = 6;
/// `m_nFlags`: body 1 is fixed.
pub const BODY1_FIXED: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub position: [f32; 3],
    /// xyzw.
    pub rotation: [f32; 4],
}

impl Default for Frame {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0] }
    }
}

/// A limit's (min, max) when enabled; angles in radians.
pub type Limit = Option<[f32; 2]>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Motor {
    pub target_velocity: [f32; 3],
    pub max: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Joint {
    pub kind: i64,
    /// Part indices.
    pub body1: usize,
    pub body2: usize,
    pub flags: i64,
    /// Each body's joint frame, local to that body.
    pub frame1: Frame,
    pub frame2: Frame,
    pub collide: bool,
    pub linear_limit: Limit,
    pub swing_limit: Limit,
    pub twist_limit: Limit,
    pub linear_motor: Option<Motor>,
    pub angular_motor: Option<Motor>,
    /// (frequency, damping ratio).
    pub linear_spring: [f32; 2],
    pub angular_spring: [f32; 2],
    pub friction: f32,
}

fn frame(v: Option<&Value>) -> Frame {
    let a: Vec<f32> = v.and_then(Value::as_array).unwrap_or_default().iter().filter_map(Value::as_f64).map(|f| f as f32).collect();
    let r = if a.len() == 7 { 3 } else { 4 };
    let at = |i: usize| a.get(i).copied().unwrap_or(0.0);
    let rotation = if a.len() > r + 3 { [at(r), at(r + 1), at(r + 2), at(r + 3)] } else { [0.0, 0.0, 0.0, 1.0] };
    Frame { position: [at(0), at(1), at(2)], rotation }
}

fn limit(o: &Object, enable: &str, key: &str) -> Limit {
    let l = kv::object(o, key);
    kv::bool(o, enable).then(|| [float(l, "m_flMin"), float(l, "m_flMax")])
}

fn motor(o: &Object, enable: &str, velocity: &str, max: &str) -> Option<Motor> {
    kv::bool(o, enable).then(|| Motor { target_velocity: kv::vec3(o, velocity), max: float(o, max) })
}

pub fn read(aggregate: &Object) -> Vec<Joint> {
    objects(aggregate, "m_joints")
        .map(|j| Joint {
            kind: int(j, "m_nType"),
            body1: int(j, "m_nBody1").max(0) as usize,
            body2: int(j, "m_nBody2").max(0) as usize,
            flags: int(j, "m_nFlags"),
            frame1: frame(j.get("m_Frame1")),
            frame2: frame(j.get("m_Frame2")),
            collide: kv::bool(j, "m_bEnableCollision"),
            linear_limit: limit(j, "m_bEnableLinearLimit", "m_LinearLimit"),
            swing_limit: limit(j, "m_bEnableSwingLimit", "m_SwingLimit"),
            twist_limit: limit(j, "m_bEnableTwistLimit", "m_TwistLimit"),
            linear_motor: motor(j, "m_bEnableLinearMotor", "m_vLinearTargetVelocity", "m_flMaxForce"),
            angular_motor: motor(j, "m_bEnableAngularMotor", "m_vAngularTargetVelocity", "m_flMaxTorque"),
            linear_spring: [float(j, "m_flLinearFrequency"), float(j, "m_flLinearDampingRatio")],
            angular_spring: [float(j, "m_flAngularFrequency"), float(j, "m_flAngularDampingRatio")],
            friction: float(j, "m_flFriction"),
        })
        .collect()
}

fn double(f: f32) -> Value {
    Value::Double(f.into())
}

pub fn write(joints: &[Joint]) -> Value {
    let frame = |f: &Frame| list(f.position.iter().chain(&[1.0]).chain(&f.rotation).map(|&v| double(v)));
    let range = |l: Limit| {
        let [min, max] = l.unwrap_or_default();
        obj(vec![("m_flMin", double(min)), ("m_flMax", double(max))])
    };
    let motor = |m: &Option<Motor>| m.clone().unwrap_or_default();
    list(joints.iter().map(|j| {
        let (lm, am) = (motor(&j.linear_motor), motor(&j.angular_motor));
        obj(vec![
            ("m_nType", int_value(j.kind)),
            ("m_nBody1", int_value(j.body1 as i64)),
            ("m_nBody2", int_value(j.body2 as i64)),
            ("m_nFlags", int_value(j.flags)),
            ("m_Frame1", frame(&j.frame1)),
            ("m_Frame2", frame(&j.frame2)),
            ("m_bEnableCollision", Value::Bool(j.collide)),
            ("m_bEnableLinearLimit", Value::Bool(j.linear_limit.is_some())),
            ("m_LinearLimit", range(j.linear_limit)),
            ("m_bEnableSwingLimit", Value::Bool(j.swing_limit.is_some())),
            ("m_SwingLimit", range(j.swing_limit)),
            ("m_bEnableTwistLimit", Value::Bool(j.twist_limit.is_some())),
            ("m_TwistLimit", range(j.twist_limit)),
            ("m_bEnableLinearMotor", Value::Bool(j.linear_motor.is_some())),
            ("m_vLinearTargetVelocity", kv::floats(&lm.target_velocity)),
            ("m_flMaxForce", double(lm.max)),
            ("m_bEnableAngularMotor", Value::Bool(j.angular_motor.is_some())),
            ("m_vAngularTargetVelocity", kv::floats(&am.target_velocity)),
            ("m_flMaxTorque", double(am.max)),
            ("m_flLinearFrequency", double(j.linear_spring[0])),
            ("m_flLinearDampingRatio", double(j.linear_spring[1])),
            ("m_flAngularFrequency", double(j.angular_spring[0])),
            ("m_flAngularDampingRatio", double(j.angular_spring[1])),
            ("m_flFriction", double(j.friction)),
        ])
    }))
}
