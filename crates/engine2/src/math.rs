//! Constructors for the ABI's math structs (which nest `System.Numerics` types), and conversions
//! to the physics crate's glam types.

use abi::structs::{Quaternion, Rotation, System_Numerics_Vector3, System_Numerics_Vector4, Transform, Vector3, Vector4};

pub fn vec3(x: f32, y: f32, z: f32) -> Vector3 {
    Vector3 { vec: System_Numerics_Vector3 { x, y, z } }
}

pub fn vec4(x: f32, y: f32, z: f32, w: f32) -> Vector4 {
    Vector4 { vec: System_Numerics_Vector4 { x, y, z, w } }
}

/// No translation, unit scale, no rotation (zeroed would be scale 0 and an invalid quaternion).
pub fn identity() -> Transform {
    Transform { position: vec3(0.0, 0.0, 0.0), scale: vec3(1.0, 1.0, 1.0), rotation: Rotation { quat: Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } } }
}

pub fn to_vec(v: Vector3) -> physics::Vector {
    physics::Vector::new(v.vec.x, v.vec.y, v.vec.z)
}

pub fn from_vec(v: physics::Vector) -> Vector3 {
    vec3(v.x, v.y, v.z)
}

/// Normalised: managed sends zeroed rotations, which Rapier can't integrate.
pub fn to_rot(r: Rotation) -> physics::Rotation {
    let q = physics::Rotation::from_xyzw(r.quat.x, r.quat.y, r.quat.z, r.quat.w);
    if q.length_squared() > 0.0 { q.normalize() } else { physics::Rotation::IDENTITY }
}

pub fn v3(v: Vector3) -> glam::Vec3 {
    glam::Vec3::new(v.vec.x, v.vec.y, v.vec.z)
}

pub fn from_v3(v: glam::Vec3) -> Vector3 {
    vec3(v.x, v.y, v.z)
}

/// Normalised, identity for a zeroed rotation (as [`to_rot`]).
pub fn quat(r: Rotation) -> glam::Quat {
    let q = glam::Quat::from_xyzw(r.quat.x, r.quat.y, r.quat.z, r.quat.w);
    if q.length_squared() > 0.0 { q.normalize() } else { glam::Quat::IDENTITY }
}

pub fn from_rot(q: physics::Rotation) -> Rotation {
    Rotation { quat: Quaternion { x: q.x, y: q.y, z: q.z, w: q.w } }
}
