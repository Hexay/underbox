//! `b2Shape_*` and `b2Chain_*`. Shape type is written out (managed's enums cross as `i64`).

use super::sys;
use abi::slots::global_box2d as s;
use abi::structs::b2ShapeId;
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, s::B2_SHAPE_GET_TYPE, get_type);
    fwd! { r:
        B2_CREATE_CIRCLE_SHAPE => b2CreateCircleShape(a; b, c);
        B2_CREATE_SEGMENT_SHAPE => b2CreateSegmentShape(a; b, c);
        B2_CREATE_CAPSULE_SHAPE => b2CreateCapsuleShape(a; b, c);
        B2_CREATE_POLYGON_SHAPE => b2CreatePolygonShape(a; b, c);
        B2_CREATE_CHAIN => b2CreateChain(a; b);
        B2_DESTROY_SHAPE => b2DestroyShape(a, b);
        B2_SHAPE_IS_VALID => b2Shape_IsValid(a);
        B2_SHAPE_GET_BODY => b2Shape_GetBody(a);
        B2_SHAPE_GET_WORLD => b2Shape_GetWorld(a);
        B2_SHAPE_IS_SENSOR => b2Shape_IsSensor(a);
        B2_SHAPE_SET_USER_DATA => b2Shape_SetUserData(a, b);
        B2_SHAPE_GET_USER_DATA => b2Shape_GetUserData(a);
        B2_SHAPE_SET_DENSITY => b2Shape_SetDensity(a, b, c);
        B2_SHAPE_GET_DENSITY => b2Shape_GetDensity(a);
        B2_SHAPE_SET_FRICTION => b2Shape_SetFriction(a, b);
        B2_SHAPE_GET_FRICTION => b2Shape_GetFriction(a);
        B2_SHAPE_SET_RESTITUTION => b2Shape_SetRestitution(a, b);
        B2_SHAPE_GET_RESTITUTION => b2Shape_GetRestitution(a);
        B2_SHAPE_SET_USER_MATERIAL => b2Shape_SetUserMaterial(a, b);
        B2_SHAPE_GET_USER_MATERIAL => b2Shape_GetUserMaterial(a);
        B2_SHAPE_SET_SURFACE_MATERIAL => b2Shape_SetSurfaceMaterial(a, b);
        B2_SHAPE_GET_SURFACE_MATERIAL => b2Shape_GetSurfaceMaterial(a);
        B2_SHAPE_GET_FILTER => b2Shape_GetFilter(a);
        B2_SHAPE_SET_FILTER => b2Shape_SetFilter(a, b);
        B2_SHAPE_ENABLE_SENSOR_EVENTS => b2Shape_EnableSensorEvents(a, b);
        B2_SHAPE_ARE_SENSOR_EVENTS_ENABLED => b2Shape_AreSensorEventsEnabled(a);
        B2_SHAPE_ENABLE_CONTACT_EVENTS => b2Shape_EnableContactEvents(a, b);
        B2_SHAPE_ARE_CONTACT_EVENTS_ENABLED => b2Shape_AreContactEventsEnabled(a);
        B2_SHAPE_ENABLE_PRE_SOLVE_EVENTS => b2Shape_EnablePreSolveEvents(a, b);
        B2_SHAPE_ARE_PRE_SOLVE_EVENTS_ENABLED => b2Shape_ArePreSolveEventsEnabled(a);
        B2_SHAPE_ENABLE_HIT_EVENTS => b2Shape_EnableHitEvents(a, b);
        B2_SHAPE_ARE_HIT_EVENTS_ENABLED => b2Shape_AreHitEventsEnabled(a);
        B2_SHAPE_TEST_POINT => b2Shape_TestPoint(a, b);
        B2_SHAPE_RAY_CAST => b2Shape_RayCast(a, b);
        B2_SHAPE_GET_CIRCLE => b2Shape_GetCircle(a);
        B2_SHAPE_GET_SEGMENT => b2Shape_GetSegment(a);
        B2_SHAPE_GET_CHAIN_SEGMENT => b2Shape_GetChainSegment(a);
        B2_SHAPE_GET_CAPSULE => b2Shape_GetCapsule(a);
        B2_SHAPE_GET_POLYGON => b2Shape_GetPolygon(a);
        B2_SHAPE_SET_CIRCLE => b2Shape_SetCircle(a, b);
        B2_SHAPE_SET_CAPSULE => b2Shape_SetCapsule(a, b);
        B2_SHAPE_SET_SEGMENT => b2Shape_SetSegment(a, b);
        B2_SHAPE_SET_POLYGON => b2Shape_SetPolygon(a, b);
        B2_SHAPE_GET_PARENT_CHAIN => b2Shape_GetParentChain(a);
        B2_SHAPE_GET_CONTACT_CAPACITY => b2Shape_GetContactCapacity(a);
        B2_SHAPE_GET_CONTACT_DATA => b2Shape_GetContactData(a, b, c);
        B2_SHAPE_GET_SENSOR_CAPACITY => b2Shape_GetSensorCapacity(a);
        B2_SHAPE_GET_SENSOR_DATA => b2Shape_GetSensorData(a, b, c);
        B2_SHAPE_GET_AABB => b2Shape_GetAABB(a);
        B2_SHAPE_COMPUTE_MASS_DATA => b2Shape_ComputeMassData(a);
        B2_SHAPE_GET_CLOSEST_POINT => b2Shape_GetClosestPoint(a, b);
        B2_SHAPE_APPLY_WIND => b2Shape_ApplyWind(a, b, c, d, e);
        B2_DESTROY_CHAIN => b2DestroyChain(a);
        B2_CHAIN_GET_WORLD => b2Chain_GetWorld(a);
        B2_CHAIN_GET_SEGMENT_COUNT => b2Chain_GetSegmentCount(a);
        B2_CHAIN_GET_SEGMENTS => b2Chain_GetSegments(a, b, c);
        B2_CHAIN_GET_SURFACE_MATERIAL_COUNT => b2Chain_GetSurfaceMaterialCount(a);
        B2_CHAIN_SET_SURFACE_MATERIAL => b2Chain_SetSurfaceMaterial(a, b, c);
        B2_CHAIN_GET_SURFACE_MATERIAL => b2Chain_GetSurfaceMaterial(a, b);
        B2_CHAIN_IS_VALID => b2Chain_IsValid(a);
    }
}

fn get_type(shape: b2ShapeId) -> i64 {
    // SAFETY: Box2D validates the id.
    i64::from(unsafe { sys::b2Shape_GetType(super::cast(shape)) })
}
