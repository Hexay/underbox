//! Readers for the source formats that describe animation: ModelDoc `.vmdl` lists, AnimGraph1
//! `.vanmgrph` parameters and tag spans, AnimGraph2 `.ag` graphs, and `.anim` sync tracks.

pub mod ag;
pub mod clip_doc;
pub mod vanmgrph;
pub mod vmdl;

use glam::{EulerRot, Quat, Vec3};
use kv::{Object, Value};

fn kv_f32(o: &Object, key: &str) -> f32 {
    o.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

fn kv_bool(o: &Object, key: &str) -> bool {
    matches!(o.get(key), Some(Value::Bool(true)))
}

fn kv_vec3(o: &Object, key: &str) -> Vec3 {
    let a = o.get(key).and_then(Value::as_array).unwrap_or_default();
    let c = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Vec3::new(c(0), c(1), c(2))
}

/// Source `QAngle` (pitch, yaw, roll in degrees) to a rotation: yaw about Z, then pitch about
/// Y, then roll about X.
pub fn angles(pyr: Vec3) -> Quat {
    Quat::from_euler(EulerRot::ZYX, pyr.y.to_radians(), pyr.x.to_radians(), pyr.z.to_radians())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_angles() {
        let yaw = angles(Vec3::new(0.0, 90.0, 0.0));
        assert!((yaw * Vec3::X).abs_diff_eq(Vec3::Y, 1e-5));
        let pitch = angles(Vec3::new(90.0, 0.0, 0.0));
        assert!((pitch * Vec3::X).abs_diff_eq(-Vec3::Z, 1e-5), "positive pitch looks down");
    }
}
