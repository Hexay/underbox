//! ABI math structs ↔ the anim crate's glam types.

use crate::math::vec3;
use abi::structs::{Quaternion, Rotation, Transform, Vector3};
use anim::Xform;
use glam::{Quat, Vec3};

pub fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.vec.x, v.vec.y, v.vec.z)
}

pub fn to_v3(v: Vec3) -> Vector3 {
    vec3(v.x, v.y, v.z)
}

/// Normalised; managed passes zeroed rotations for "none".
pub fn quat(r: Rotation) -> Quat {
    let q = Quat::from_xyzw(r.quat.x, r.quat.y, r.quat.z, r.quat.w);
    if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY }
}

pub fn to_rot(q: Quat) -> Rotation {
    Rotation { quat: Quaternion { x: q.x, y: q.y, z: q.z, w: q.w } }
}

/// Scale is uniform on bones; a zero (unset) managed scale reads as 1.
pub fn xform(t: Transform) -> Xform {
    let s = t.scale.vec.x;
    Xform { pos: v3(t.position), rot: quat(t.rotation), scale: if s == 0.0 { 1.0 } else { s } }
}

pub fn to_transform(x: &Xform) -> Transform {
    Transform { position: to_v3(x.pos), scale: vec3(x.scale, x.scale, x.scale), rotation: to_rot(x.rot) }
}
