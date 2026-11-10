//! Parameter values across the ABI: AnimGraph1's `AnimVariant` and type codes, and AnimGraph2's
//! `GraphValueKind` codes (AnimationAccessor.cs).

use super::conv::quat;
use crate::math::vec4;
use abi::structs::{AnimVariant, Quaternion, Rotation};
use anim::params::Value;

/// `AnimParamType` (AnimationGraph.Native.cs): Unknown, Bool, Enum, Int, Float, Vector, Rotation.
pub fn legacy_type(v: &Value) -> i64 {
    match v {
        Value::Bool(_) => 1,
        Value::Enum(_) => 2,
        Value::Int(_) => 3,
        Value::Float(_) => 4,
        Value::Vector(_) => 5,
        Value::Rotation(_) => 6,
        Value::Id(_) | Value::Target(_) => 0,
    }
}

/// `GraphValueKind`: Unknown, Bool, Id, Float, Vector, Target, BoneMask, Pose.
pub fn graph_kind(v: &Value) -> i64 {
    match v {
        Value::Bool(_) => 1,
        Value::Id(_) => 2,
        Value::Float(_) | Value::Int(_) | Value::Enum(_) => 3,
        Value::Vector(_) => 4,
        Value::Target(_) | Value::Rotation(_) => 5,
    }
}

pub fn to_variant(v: &Value) -> AnimVariant {
    let mut out = AnimVariant::default();
    out.set_type_(legacy_type(v) as u8);
    match v {
        Value::Bool(b) => out.set_integer_value(*b as i32),
        Value::Enum(e) => out.set_integer_value(*e as i32),
        Value::Int(i) => out.set_integer_value(*i),
        Value::Float(f) => out.set_raw_value(vec4(*f, 0.0, 0.0, 0.0)),
        Value::Vector(p) => out.set_raw_value(vec4(p.x, p.y, p.z, 0.0)),
        Value::Rotation(q) => out.set_raw_value(vec4(q.x, q.y, q.z, q.w)),
        Value::Id(_) | Value::Target(_) => {}
    }
    out
}

pub fn from_variant(v: AnimVariant) -> Option<Value> {
    let raw = v.raw_value().vec;
    Some(match v.type_() {
        1 => Value::Bool(v.integer_value() as u8 != 0),
        2 => Value::Enum(v.integer_value() as u8),
        3 => Value::Int(v.integer_value()),
        4 => Value::Float(raw.x),
        5 => Value::Vector(glam::Vec3::new(raw.x, raw.y, raw.z)),
        6 => Value::Rotation(quat(Rotation { quat: Quaternion { x: raw.x, y: raw.y, z: raw.z, w: raw.w } })),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_round_trip() {
        for v in [Value::Bool(true), Value::Enum(3), Value::Int(-7), Value::Float(2.5), Value::Vector(glam::Vec3::new(1.0, 2.0, 3.0))] {
            assert_eq!(from_variant(to_variant(&v)), Some(v));
        }
        assert_eq!(from_variant(AnimVariant::default()), None, "type 0 is Unknown");
    }
}
