//! `b2Body_*`. Type and name are written out: managed's enums cross as `i64` and its strings as C
//! strings, where Box2D has a C `int` and a `const char*` that lives in the body.

use super::sys;
use abi::slots::global_box2d as s;
use abi::structs::b2BodyId;
use abi::{Registry, StrRet, bind};
use std::ffi::CStr;

pub fn register(r: &mut Registry) {
    bind!(r, s::B2_BODY_GET_TYPE, get_type);
    bind!(r, s::B2_BODY_SET_TYPE, set_type);
    bind!(r, s::B2_BODY_SET_NAME, set_name);
    bind!(r, s::B2_BODY_GET_NAME, get_name);
    fwd! { r:
        B2_CREATE_BODY => b2CreateBody(a; b);
        B2_DESTROY_BODY => b2DestroyBody(a);
        B2_BODY_IS_VALID => b2Body_IsValid(a);
        B2_BODY_SET_USER_DATA => b2Body_SetUserData(a, b);
        B2_BODY_GET_USER_DATA => b2Body_GetUserData(a);
        B2_BODY_GET_POSITION => b2Body_GetPosition(a);
        B2_BODY_GET_ROTATION => b2Body_GetRotation(a);
        B2_BODY_GET_TRANSFORM => b2Body_GetTransform(a);
        B2_BODY_SET_TRANSFORM => b2Body_SetTransform(a, b, c);
        B2_BODY_GET_LOCAL_POINT => b2Body_GetLocalPoint(a, b);
        B2_BODY_GET_WORLD_POINT => b2Body_GetWorldPoint(a, b);
        B2_BODY_GET_LOCAL_VECTOR => b2Body_GetLocalVector(a, b);
        B2_BODY_GET_WORLD_VECTOR => b2Body_GetWorldVector(a, b);
        B2_BODY_GET_LINEAR_VELOCITY => b2Body_GetLinearVelocity(a);
        B2_BODY_GET_ANGULAR_VELOCITY => b2Body_GetAngularVelocity(a);
        B2_BODY_SET_LINEAR_VELOCITY => b2Body_SetLinearVelocity(a, b);
        B2_BODY_SET_ANGULAR_VELOCITY => b2Body_SetAngularVelocity(a, b);
        B2_BODY_SET_TARGET_TRANSFORM => b2Body_SetTargetTransform(a, b, c, d);
        B2_BODY_GET_LOCAL_POINT_VELOCITY => b2Body_GetLocalPointVelocity(a, b);
        B2_BODY_GET_WORLD_POINT_VELOCITY => b2Body_GetWorldPointVelocity(a, b);
        B2_BODY_APPLY_FORCE => b2Body_ApplyForce(a, b, c, d);
        B2_BODY_APPLY_FORCE_TO_CENTER => b2Body_ApplyForceToCenter(a, b, c);
        B2_BODY_APPLY_TORQUE => b2Body_ApplyTorque(a, b, c);
        B2_BODY_CLEAR_FORCES => b2Body_ClearForces(a);
        B2_BODY_APPLY_LINEAR_IMPULSE => b2Body_ApplyLinearImpulse(a, b, c, d);
        B2_BODY_APPLY_LINEAR_IMPULSE_TO_CENTER => b2Body_ApplyLinearImpulseToCenter(a, b, c);
        B2_BODY_APPLY_ANGULAR_IMPULSE => b2Body_ApplyAngularImpulse(a, b, c);
        B2_BODY_GET_MASS => b2Body_GetMass(a);
        B2_BODY_GET_ROTATIONAL_INERTIA => b2Body_GetRotationalInertia(a);
        B2_BODY_GET_LOCAL_CENTER_OF_MASS => b2Body_GetLocalCenterOfMass(a);
        B2_BODY_GET_WORLD_CENTER_OF_MASS => b2Body_GetWorldCenterOfMass(a);
        B2_BODY_SET_MASS_DATA => b2Body_SetMassData(a, b);
        B2_BODY_GET_MASS_DATA => b2Body_GetMassData(a);
        B2_BODY_APPLY_MASS_FROM_SHAPES => b2Body_ApplyMassFromShapes(a);
        B2_BODY_SET_LINEAR_DAMPING => b2Body_SetLinearDamping(a, b);
        B2_BODY_GET_LINEAR_DAMPING => b2Body_GetLinearDamping(a);
        B2_BODY_SET_ANGULAR_DAMPING => b2Body_SetAngularDamping(a, b);
        B2_BODY_GET_ANGULAR_DAMPING => b2Body_GetAngularDamping(a);
        B2_BODY_SET_GRAVITY_SCALE => b2Body_SetGravityScale(a, b);
        B2_BODY_GET_GRAVITY_SCALE => b2Body_GetGravityScale(a);
        B2_BODY_IS_AWAKE => b2Body_IsAwake(a);
        B2_BODY_SET_AWAKE => b2Body_SetAwake(a, b);
        B2_BODY_WAKE_TOUCHING => b2Body_WakeTouching(a);
        B2_BODY_ENABLE_SLEEP => b2Body_EnableSleep(a, b);
        B2_BODY_IS_SLEEP_ENABLED => b2Body_IsSleepEnabled(a);
        B2_BODY_SET_SLEEP_THRESHOLD => b2Body_SetSleepThreshold(a, b);
        B2_BODY_GET_SLEEP_THRESHOLD => b2Body_GetSleepThreshold(a);
        B2_BODY_IS_ENABLED => b2Body_IsEnabled(a);
        B2_BODY_DISABLE => b2Body_Disable(a);
        B2_BODY_ENABLE => b2Body_Enable(a);
        B2_BODY_SET_MOTION_LOCKS => b2Body_SetMotionLocks(a, b);
        B2_BODY_GET_MOTION_LOCKS => b2Body_GetMotionLocks(a);
        B2_BODY_SET_BULLET => b2Body_SetBullet(a, b);
        B2_BODY_IS_BULLET => b2Body_IsBullet(a);
        B2_BODY_ENABLE_CONTACT_EVENTS => b2Body_EnableContactEvents(a, b);
        B2_BODY_ENABLE_HIT_EVENTS => b2Body_EnableHitEvents(a, b);
        B2_BODY_GET_WORLD => b2Body_GetWorld(a);
        B2_BODY_GET_SHAPE_COUNT => b2Body_GetShapeCount(a);
        B2_BODY_GET_SHAPES => b2Body_GetShapes(a, b, c);
        B2_BODY_GET_JOINT_COUNT => b2Body_GetJointCount(a);
        B2_BODY_GET_JOINTS => b2Body_GetJoints(a, b, c);
        B2_BODY_GET_CONTACT_CAPACITY => b2Body_GetContactCapacity(a);
        B2_BODY_GET_CONTACT_DATA => b2Body_GetContactData(a, b, c);
        B2_BODY_COMPUTE_AABB => b2Body_ComputeAABB(a);
    }
}

fn get_type(body: b2BodyId) -> i64 {
    // SAFETY: Box2D validates the id.
    i64::from(unsafe { sys::b2Body_GetType(super::cast(body)) })
}

fn set_type(body: b2BodyId, kind: i64) {
    // SAFETY: as above; Box2D asserts the type is in range.
    unsafe { sys::b2Body_SetType(super::cast(body), kind as sys::b2BodyType) }
}

fn set_name(body: b2BodyId, name: Option<&CStr>) {
    // SAFETY: Box2D copies the name into the body (B2_NAME_LENGTH bytes, truncating).
    unsafe { sys::b2Body_SetName(super::cast(body), name.map_or(std::ptr::null(), CStr::as_ptr)) }
}

fn get_name(body: b2BodyId) -> StrRet {
    // SAFETY: Box2D returns the body's own NUL-terminated buffer, valid until the body changes.
    let name = unsafe { sys::b2Body_GetName(super::cast(body)) };
    if name.is_null() {
        return StrRet::Null;
    }
    StrRet::from(unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned())
}
