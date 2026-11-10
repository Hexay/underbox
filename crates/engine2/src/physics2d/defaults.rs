//! Default defs, geometry helpers, contacts and the (process-wide) length unit.

use abi::Registry;

pub fn register(r: &mut Registry) {
    fwd! { r:
        B2_SET_LENGTH_UNITS_PER_METER => b2SetLengthUnitsPerMeter(a);
        B2_GET_LENGTH_UNITS_PER_METER => b2GetLengthUnitsPerMeter();
        B2_CONTACT_IS_VALID => b2Contact_IsValid(a);
        B2_CONTACT_GET_DATA => b2Contact_GetData(a);
        B2_DEFAULT_WORLD_DEF => b2DefaultWorldDef();
        B2_DEFAULT_BODY_DEF => b2DefaultBodyDef();
        B2_DEFAULT_SHAPE_DEF => b2DefaultShapeDef();
        B2_DEFAULT_CHAIN_DEF => b2DefaultChainDef();
        B2_DEFAULT_DISTANCE_JOINT_DEF => b2DefaultDistanceJointDef();
        B2_DEFAULT_MOTOR_JOINT_DEF => b2DefaultMotorJointDef();
        B2_DEFAULT_FILTER_JOINT_DEF => b2DefaultFilterJointDef();
        B2_DEFAULT_PRISMATIC_JOINT_DEF => b2DefaultPrismaticJointDef();
        B2_DEFAULT_REVOLUTE_JOINT_DEF => b2DefaultRevoluteJointDef();
        B2_DEFAULT_WELD_JOINT_DEF => b2DefaultWeldJointDef();
        B2_DEFAULT_WHEEL_JOINT_DEF => b2DefaultWheelJointDef();
        B2_DEFAULT_EXPLOSION_DEF => b2DefaultExplosionDef();
        B2_DEFAULT_FILTER => b2DefaultFilter();
        B2_DEFAULT_QUERY_FILTER => b2DefaultQueryFilter();
        B2_DEFAULT_SURFACE_MATERIAL => b2DefaultSurfaceMaterial();
        B2_MAKE_BOX => b2MakeBox(a, b);
        B2_MAKE_OFFSET_BOX => b2MakeOffsetBox(a, b, c, d);
        B2_COMPUTE_HULL => b2ComputeHull(a, b);
        B2_MAKE_POLYGON => b2MakePolygon(a, b);
        B2_MAKE_OFFSET_POLYGON => b2MakeOffsetPolygon(a, b, c);
    }
}
