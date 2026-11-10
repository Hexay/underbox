//! `b2World_*`: lifetime, stepping, events, queries and tuning.

use abi::Registry;

pub fn register(r: &mut Registry) {
    fwd! { r:
        B2_CREATE_WORLD => b2CreateWorld(; a);
        B2_WORLD_EXPLODE => b2World_Explode(a; b);
        B2_DESTROY_WORLD => b2DestroyWorld(a);
        B2_WORLD_IS_VALID => b2World_IsValid(a);
        B2_WORLD_STEP => b2World_Step(a, b, c);
        B2_WORLD_DRAW => b2World_Draw(a, b);
        B2_WORLD_GET_BODY_EVENTS => b2World_GetBodyEvents(a);
        B2_WORLD_GET_SENSOR_EVENTS => b2World_GetSensorEvents(a);
        B2_WORLD_GET_CONTACT_EVENTS => b2World_GetContactEvents(a);
        B2_WORLD_GET_JOINT_EVENTS => b2World_GetJointEvents(a);
        B2_WORLD_OVERLAP_AABB => b2World_OverlapAABB(a, b, c, d, e);
        B2_WORLD_OVERLAP_SHAPE => b2World_OverlapShape(a, b, c, d, e);
        B2_WORLD_CAST_RAY => b2World_CastRay(a, b, c, d, e, f);
        B2_WORLD_CAST_RAY_CLOSEST => b2World_CastRayClosest(a, b, c, d);
        B2_WORLD_CAST_SHAPE => b2World_CastShape(a, b, c, d, e, f);
        B2_WORLD_CAST_MOVER => b2World_CastMover(a, b, c, d);
        B2_WORLD_COLLIDE_MOVER => b2World_CollideMover(a, b, c, d, e);
        B2_WORLD_ENABLE_SLEEPING => b2World_EnableSleeping(a, b);
        B2_WORLD_IS_SLEEPING_ENABLED => b2World_IsSleepingEnabled(a);
        B2_WORLD_ENABLE_CONTINUOUS => b2World_EnableContinuous(a, b);
        B2_WORLD_IS_CONTINUOUS_ENABLED => b2World_IsContinuousEnabled(a);
        B2_WORLD_SET_RESTITUTION_THRESHOLD => b2World_SetRestitutionThreshold(a, b);
        B2_WORLD_GET_RESTITUTION_THRESHOLD => b2World_GetRestitutionThreshold(a);
        B2_WORLD_SET_HIT_EVENT_THRESHOLD => b2World_SetHitEventThreshold(a, b);
        B2_WORLD_GET_HIT_EVENT_THRESHOLD => b2World_GetHitEventThreshold(a);
        B2_WORLD_SET_CUSTOM_FILTER_CALLBACK => b2World_SetCustomFilterCallback(a, b, c);
        B2_WORLD_SET_PRE_SOLVE_CALLBACK => b2World_SetPreSolveCallback(a, b, c);
        B2_WORLD_SET_GRAVITY => b2World_SetGravity(a, b);
        B2_WORLD_GET_GRAVITY => b2World_GetGravity(a);
        B2_WORLD_SET_CONTACT_TUNING => b2World_SetContactTuning(a, b, c, d);
        B2_WORLD_SET_CONTACT_RECYCLE_DISTANCE => b2World_SetContactRecycleDistance(a, b);
        B2_WORLD_GET_CONTACT_RECYCLE_DISTANCE => b2World_GetContactRecycleDistance(a);
        B2_WORLD_SET_MAXIMUM_LINEAR_SPEED => b2World_SetMaximumLinearSpeed(a, b);
        B2_WORLD_GET_MAXIMUM_LINEAR_SPEED => b2World_GetMaximumLinearSpeed(a);
        B2_WORLD_ENABLE_WARM_STARTING => b2World_EnableWarmStarting(a, b);
        B2_WORLD_IS_WARM_STARTING_ENABLED => b2World_IsWarmStartingEnabled(a);
        B2_WORLD_GET_AWAKE_BODY_COUNT => b2World_GetAwakeBodyCount(a);
        B2_WORLD_GET_PROFILE => b2World_GetProfile(a);
        B2_WORLD_GET_COUNTERS => b2World_GetCounters(a);
        B2_WORLD_SET_USER_DATA => b2World_SetUserData(a, b);
        B2_WORLD_GET_USER_DATA => b2World_GetUserData(a);
        B2_WORLD_SET_FRICTION_CALLBACK => b2World_SetFrictionCallback(a, b);
        B2_WORLD_SET_RESTITUTION_CALLBACK => b2World_SetRestitutionCallback(a, b);
        B2_WORLD_DUMP_MEMORY_STATS => b2World_DumpMemoryStats(a);
        B2_WORLD_REBUILD_STATIC_TREE => b2World_RebuildStaticTree(a);
        B2_WORLD_ENABLE_SPECULATIVE => b2World_EnableSpeculative(a, b);
        B2_WORLD_GET_MAX_CAPACITY => b2World_GetMaxCapacity(a);
        B2_WORLD_GET_WORKER_COUNT => b2World_GetWorkerCount(a);
        B2_WORLD_SET_WORKER_COUNT => b2World_SetWorkerCount(a, b);
    }
}
