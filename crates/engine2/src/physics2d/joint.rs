//! Joint creation, `b2Joint_*`, `b2DistanceJoint_*` and `b2MotorJoint_*`. Joint type is written
//! out (managed's enums cross as `i64`).

use super::sys;
use abi::slots::global_box2d as s;
use abi::structs::b2JointId;
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, s::B2_JOINT_GET_TYPE, get_type);
    fwd! { r:
        B2_CREATE_DISTANCE_JOINT => b2CreateDistanceJoint(a; b);
        B2_CREATE_MOTOR_JOINT => b2CreateMotorJoint(a; b);
        B2_CREATE_FILTER_JOINT => b2CreateFilterJoint(a; b);
        B2_CREATE_PRISMATIC_JOINT => b2CreatePrismaticJoint(a; b);
        B2_CREATE_REVOLUTE_JOINT => b2CreateRevoluteJoint(a; b);
        B2_CREATE_WELD_JOINT => b2CreateWeldJoint(a; b);
        B2_CREATE_WHEEL_JOINT => b2CreateWheelJoint(a; b);
        B2_DESTROY_JOINT => b2DestroyJoint(a, b);
        B2_JOINT_IS_VALID => b2Joint_IsValid(a);
        B2_JOINT_GET_BODY_A => b2Joint_GetBodyA(a);
        B2_JOINT_GET_BODY_B => b2Joint_GetBodyB(a);
        B2_JOINT_GET_WORLD => b2Joint_GetWorld(a);
        B2_JOINT_SET_LOCAL_FRAME_A => b2Joint_SetLocalFrameA(a, b);
        B2_JOINT_GET_LOCAL_FRAME_A => b2Joint_GetLocalFrameA(a);
        B2_JOINT_SET_LOCAL_FRAME_B => b2Joint_SetLocalFrameB(a, b);
        B2_JOINT_GET_LOCAL_FRAME_B => b2Joint_GetLocalFrameB(a);
        B2_JOINT_SET_COLLIDE_CONNECTED => b2Joint_SetCollideConnected(a, b);
        B2_JOINT_GET_COLLIDE_CONNECTED => b2Joint_GetCollideConnected(a);
        B2_JOINT_SET_USER_DATA => b2Joint_SetUserData(a, b);
        B2_JOINT_GET_USER_DATA => b2Joint_GetUserData(a);
        B2_JOINT_WAKE_BODIES => b2Joint_WakeBodies(a);
        B2_JOINT_GET_CONSTRAINT_FORCE => b2Joint_GetConstraintForce(a);
        B2_JOINT_GET_CONSTRAINT_TORQUE => b2Joint_GetConstraintTorque(a);
        B2_JOINT_GET_LINEAR_SEPARATION => b2Joint_GetLinearSeparation(a);
        B2_JOINT_GET_ANGULAR_SEPARATION => b2Joint_GetAngularSeparation(a);
        B2_JOINT_SET_CONSTRAINT_TUNING => b2Joint_SetConstraintTuning(a, b, c);
        B2_JOINT_GET_CONSTRAINT_TUNING => b2Joint_GetConstraintTuning(a, b, c);
        B2_JOINT_SET_FORCE_THRESHOLD => b2Joint_SetForceThreshold(a, b);
        B2_JOINT_GET_FORCE_THRESHOLD => b2Joint_GetForceThreshold(a);
        B2_JOINT_SET_TORQUE_THRESHOLD => b2Joint_SetTorqueThreshold(a, b);
        B2_JOINT_GET_TORQUE_THRESHOLD => b2Joint_GetTorqueThreshold(a);
        B2_DISTANCE_JOINT_SET_LENGTH => b2DistanceJoint_SetLength(a, b);
        B2_DISTANCE_JOINT_GET_LENGTH => b2DistanceJoint_GetLength(a);
        B2_DISTANCE_JOINT_ENABLE_SPRING => b2DistanceJoint_EnableSpring(a, b);
        B2_DISTANCE_JOINT_IS_SPRING_ENABLED => b2DistanceJoint_IsSpringEnabled(a);
        B2_DISTANCE_JOINT_SET_SPRING_FORCE_RANGE => b2DistanceJoint_SetSpringForceRange(a, b, c);
        B2_DISTANCE_JOINT_GET_SPRING_FORCE_RANGE => b2DistanceJoint_GetSpringForceRange(a, b, c);
        B2_DISTANCE_JOINT_SET_SPRING_HERTZ => b2DistanceJoint_SetSpringHertz(a, b);
        B2_DISTANCE_JOINT_SET_SPRING_DAMPING_RATIO => b2DistanceJoint_SetSpringDampingRatio(a, b);
        B2_DISTANCE_JOINT_GET_SPRING_HERTZ => b2DistanceJoint_GetSpringHertz(a);
        B2_DISTANCE_JOINT_GET_SPRING_DAMPING_RATIO => b2DistanceJoint_GetSpringDampingRatio(a);
        B2_DISTANCE_JOINT_ENABLE_LIMIT => b2DistanceJoint_EnableLimit(a, b);
        B2_DISTANCE_JOINT_IS_LIMIT_ENABLED => b2DistanceJoint_IsLimitEnabled(a);
        B2_DISTANCE_JOINT_SET_LENGTH_RANGE => b2DistanceJoint_SetLengthRange(a, b, c);
        B2_DISTANCE_JOINT_GET_MIN_LENGTH => b2DistanceJoint_GetMinLength(a);
        B2_DISTANCE_JOINT_GET_MAX_LENGTH => b2DistanceJoint_GetMaxLength(a);
        B2_DISTANCE_JOINT_GET_CURRENT_LENGTH => b2DistanceJoint_GetCurrentLength(a);
        B2_DISTANCE_JOINT_ENABLE_MOTOR => b2DistanceJoint_EnableMotor(a, b);
        B2_DISTANCE_JOINT_IS_MOTOR_ENABLED => b2DistanceJoint_IsMotorEnabled(a);
        B2_DISTANCE_JOINT_SET_MOTOR_SPEED => b2DistanceJoint_SetMotorSpeed(a, b);
        B2_DISTANCE_JOINT_GET_MOTOR_SPEED => b2DistanceJoint_GetMotorSpeed(a);
        B2_DISTANCE_JOINT_SET_MAX_MOTOR_FORCE => b2DistanceJoint_SetMaxMotorForce(a, b);
        B2_DISTANCE_JOINT_GET_MAX_MOTOR_FORCE => b2DistanceJoint_GetMaxMotorForce(a);
        B2_DISTANCE_JOINT_GET_MOTOR_FORCE => b2DistanceJoint_GetMotorForce(a);
        B2_MOTOR_JOINT_SET_LINEAR_VELOCITY => b2MotorJoint_SetLinearVelocity(a, b);
        B2_MOTOR_JOINT_GET_LINEAR_VELOCITY => b2MotorJoint_GetLinearVelocity(a);
        B2_MOTOR_JOINT_SET_ANGULAR_VELOCITY => b2MotorJoint_SetAngularVelocity(a, b);
        B2_MOTOR_JOINT_GET_ANGULAR_VELOCITY => b2MotorJoint_GetAngularVelocity(a);
        B2_MOTOR_JOINT_SET_MAX_VELOCITY_FORCE => b2MotorJoint_SetMaxVelocityForce(a, b);
        B2_MOTOR_JOINT_GET_MAX_VELOCITY_FORCE => b2MotorJoint_GetMaxVelocityForce(a);
        B2_MOTOR_JOINT_SET_MAX_VELOCITY_TORQUE => b2MotorJoint_SetMaxVelocityTorque(a, b);
        B2_MOTOR_JOINT_GET_MAX_VELOCITY_TORQUE => b2MotorJoint_GetMaxVelocityTorque(a);
        B2_MOTOR_JOINT_SET_LINEAR_HERTZ => b2MotorJoint_SetLinearHertz(a, b);
        B2_MOTOR_JOINT_GET_LINEAR_HERTZ => b2MotorJoint_GetLinearHertz(a);
        B2_MOTOR_JOINT_SET_LINEAR_DAMPING_RATIO => b2MotorJoint_SetLinearDampingRatio(a, b);
        B2_MOTOR_JOINT_GET_LINEAR_DAMPING_RATIO => b2MotorJoint_GetLinearDampingRatio(a);
        B2_MOTOR_JOINT_SET_ANGULAR_HERTZ => b2MotorJoint_SetAngularHertz(a, b);
        B2_MOTOR_JOINT_GET_ANGULAR_HERTZ => b2MotorJoint_GetAngularHertz(a);
        B2_MOTOR_JOINT_SET_ANGULAR_DAMPING_RATIO => b2MotorJoint_SetAngularDampingRatio(a, b);
        B2_MOTOR_JOINT_GET_ANGULAR_DAMPING_RATIO => b2MotorJoint_GetAngularDampingRatio(a);
        B2_MOTOR_JOINT_SET_MAX_SPRING_FORCE => b2MotorJoint_SetMaxSpringForce(a, b);
        B2_MOTOR_JOINT_GET_MAX_SPRING_FORCE => b2MotorJoint_GetMaxSpringForce(a);
        B2_MOTOR_JOINT_SET_MAX_SPRING_TORQUE => b2MotorJoint_SetMaxSpringTorque(a, b);
        B2_MOTOR_JOINT_GET_MAX_SPRING_TORQUE => b2MotorJoint_GetMaxSpringTorque(a);
    }
}

fn get_type(joint: b2JointId) -> i64 {
    // SAFETY: Box2D validates the id.
    i64::from(unsafe { sys::b2Joint_GetType(super::cast(joint)) })
}
