//! `b2PrismaticJoint_*`, `b2RevoluteJoint_*`, `b2WeldJoint_*` and `b2WheelJoint_*`.

use abi::Registry;

pub fn register(r: &mut Registry) {
    fwd! { r:
        B2_PRISMATIC_JOINT_ENABLE_SPRING => b2PrismaticJoint_EnableSpring(a, b);
        B2_PRISMATIC_JOINT_IS_SPRING_ENABLED => b2PrismaticJoint_IsSpringEnabled(a);
        B2_PRISMATIC_JOINT_SET_SPRING_HERTZ => b2PrismaticJoint_SetSpringHertz(a, b);
        B2_PRISMATIC_JOINT_GET_SPRING_HERTZ => b2PrismaticJoint_GetSpringHertz(a);
        B2_PRISMATIC_JOINT_SET_SPRING_DAMPING_RATIO => b2PrismaticJoint_SetSpringDampingRatio(a, b);
        B2_PRISMATIC_JOINT_GET_SPRING_DAMPING_RATIO => b2PrismaticJoint_GetSpringDampingRatio(a);
        B2_PRISMATIC_JOINT_SET_TARGET_TRANSLATION => b2PrismaticJoint_SetTargetTranslation(a, b);
        B2_PRISMATIC_JOINT_GET_TARGET_TRANSLATION => b2PrismaticJoint_GetTargetTranslation(a);
        B2_PRISMATIC_JOINT_ENABLE_LIMIT => b2PrismaticJoint_EnableLimit(a, b);
        B2_PRISMATIC_JOINT_IS_LIMIT_ENABLED => b2PrismaticJoint_IsLimitEnabled(a);
        B2_PRISMATIC_JOINT_GET_LOWER_LIMIT => b2PrismaticJoint_GetLowerLimit(a);
        B2_PRISMATIC_JOINT_GET_UPPER_LIMIT => b2PrismaticJoint_GetUpperLimit(a);
        B2_PRISMATIC_JOINT_SET_LIMITS => b2PrismaticJoint_SetLimits(a, b, c);
        B2_PRISMATIC_JOINT_ENABLE_MOTOR => b2PrismaticJoint_EnableMotor(a, b);
        B2_PRISMATIC_JOINT_IS_MOTOR_ENABLED => b2PrismaticJoint_IsMotorEnabled(a);
        B2_PRISMATIC_JOINT_SET_MOTOR_SPEED => b2PrismaticJoint_SetMotorSpeed(a, b);
        B2_PRISMATIC_JOINT_GET_MOTOR_SPEED => b2PrismaticJoint_GetMotorSpeed(a);
        B2_PRISMATIC_JOINT_SET_MAX_MOTOR_FORCE => b2PrismaticJoint_SetMaxMotorForce(a, b);
        B2_PRISMATIC_JOINT_GET_MAX_MOTOR_FORCE => b2PrismaticJoint_GetMaxMotorForce(a);
        B2_PRISMATIC_JOINT_GET_MOTOR_FORCE => b2PrismaticJoint_GetMotorForce(a);
        B2_PRISMATIC_JOINT_GET_TRANSLATION => b2PrismaticJoint_GetTranslation(a);
        B2_PRISMATIC_JOINT_GET_SPEED => b2PrismaticJoint_GetSpeed(a);
        B2_REVOLUTE_JOINT_ENABLE_SPRING => b2RevoluteJoint_EnableSpring(a, b);
        B2_REVOLUTE_JOINT_IS_SPRING_ENABLED => b2RevoluteJoint_IsSpringEnabled(a);
        B2_REVOLUTE_JOINT_SET_SPRING_HERTZ => b2RevoluteJoint_SetSpringHertz(a, b);
        B2_REVOLUTE_JOINT_GET_SPRING_HERTZ => b2RevoluteJoint_GetSpringHertz(a);
        B2_REVOLUTE_JOINT_SET_SPRING_DAMPING_RATIO => b2RevoluteJoint_SetSpringDampingRatio(a, b);
        B2_REVOLUTE_JOINT_GET_SPRING_DAMPING_RATIO => b2RevoluteJoint_GetSpringDampingRatio(a);
        B2_REVOLUTE_JOINT_SET_TARGET_ANGLE => b2RevoluteJoint_SetTargetAngle(a, b);
        B2_REVOLUTE_JOINT_GET_TARGET_ANGLE => b2RevoluteJoint_GetTargetAngle(a);
        B2_REVOLUTE_JOINT_GET_ANGLE => b2RevoluteJoint_GetAngle(a);
        B2_REVOLUTE_JOINT_ENABLE_LIMIT => b2RevoluteJoint_EnableLimit(a, b);
        B2_REVOLUTE_JOINT_IS_LIMIT_ENABLED => b2RevoluteJoint_IsLimitEnabled(a);
        B2_REVOLUTE_JOINT_GET_LOWER_LIMIT => b2RevoluteJoint_GetLowerLimit(a);
        B2_REVOLUTE_JOINT_GET_UPPER_LIMIT => b2RevoluteJoint_GetUpperLimit(a);
        B2_REVOLUTE_JOINT_SET_LIMITS => b2RevoluteJoint_SetLimits(a, b, c);
        B2_REVOLUTE_JOINT_ENABLE_MOTOR => b2RevoluteJoint_EnableMotor(a, b);
        B2_REVOLUTE_JOINT_IS_MOTOR_ENABLED => b2RevoluteJoint_IsMotorEnabled(a);
        B2_REVOLUTE_JOINT_SET_MOTOR_SPEED => b2RevoluteJoint_SetMotorSpeed(a, b);
        B2_REVOLUTE_JOINT_GET_MOTOR_SPEED => b2RevoluteJoint_GetMotorSpeed(a);
        B2_REVOLUTE_JOINT_GET_MOTOR_TORQUE => b2RevoluteJoint_GetMotorTorque(a);
        B2_REVOLUTE_JOINT_SET_MAX_MOTOR_TORQUE => b2RevoluteJoint_SetMaxMotorTorque(a, b);
        B2_REVOLUTE_JOINT_GET_MAX_MOTOR_TORQUE => b2RevoluteJoint_GetMaxMotorTorque(a);
        B2_WELD_JOINT_SET_LINEAR_HERTZ => b2WeldJoint_SetLinearHertz(a, b);
        B2_WELD_JOINT_GET_LINEAR_HERTZ => b2WeldJoint_GetLinearHertz(a);
        B2_WELD_JOINT_SET_LINEAR_DAMPING_RATIO => b2WeldJoint_SetLinearDampingRatio(a, b);
        B2_WELD_JOINT_GET_LINEAR_DAMPING_RATIO => b2WeldJoint_GetLinearDampingRatio(a);
        B2_WELD_JOINT_SET_ANGULAR_HERTZ => b2WeldJoint_SetAngularHertz(a, b);
        B2_WELD_JOINT_GET_ANGULAR_HERTZ => b2WeldJoint_GetAngularHertz(a);
        B2_WELD_JOINT_SET_ANGULAR_DAMPING_RATIO => b2WeldJoint_SetAngularDampingRatio(a, b);
        B2_WELD_JOINT_GET_ANGULAR_DAMPING_RATIO => b2WeldJoint_GetAngularDampingRatio(a);
        B2_WHEEL_JOINT_ENABLE_SPRING => b2WheelJoint_EnableSpring(a, b);
        B2_WHEEL_JOINT_IS_SPRING_ENABLED => b2WheelJoint_IsSpringEnabled(a);
        B2_WHEEL_JOINT_SET_SPRING_HERTZ => b2WheelJoint_SetSpringHertz(a, b);
        B2_WHEEL_JOINT_GET_SPRING_HERTZ => b2WheelJoint_GetSpringHertz(a);
        B2_WHEEL_JOINT_SET_SPRING_DAMPING_RATIO => b2WheelJoint_SetSpringDampingRatio(a, b);
        B2_WHEEL_JOINT_GET_SPRING_DAMPING_RATIO => b2WheelJoint_GetSpringDampingRatio(a);
        B2_WHEEL_JOINT_ENABLE_LIMIT => b2WheelJoint_EnableLimit(a, b);
        B2_WHEEL_JOINT_IS_LIMIT_ENABLED => b2WheelJoint_IsLimitEnabled(a);
        B2_WHEEL_JOINT_GET_LOWER_LIMIT => b2WheelJoint_GetLowerLimit(a);
        B2_WHEEL_JOINT_GET_UPPER_LIMIT => b2WheelJoint_GetUpperLimit(a);
        B2_WHEEL_JOINT_SET_LIMITS => b2WheelJoint_SetLimits(a, b, c);
        B2_WHEEL_JOINT_ENABLE_MOTOR => b2WheelJoint_EnableMotor(a, b);
        B2_WHEEL_JOINT_IS_MOTOR_ENABLED => b2WheelJoint_IsMotorEnabled(a);
        B2_WHEEL_JOINT_SET_MOTOR_SPEED => b2WheelJoint_SetMotorSpeed(a, b);
        B2_WHEEL_JOINT_GET_MOTOR_SPEED => b2WheelJoint_GetMotorSpeed(a);
        B2_WHEEL_JOINT_SET_MAX_MOTOR_TORQUE => b2WheelJoint_SetMaxMotorTorque(a, b);
        B2_WHEEL_JOINT_GET_MAX_MOTOR_TORQUE => b2WheelJoint_GetMaxMotorTorque(a);
        B2_WHEEL_JOINT_GET_MOTOR_TORQUE => b2WheelJoint_GetMotorTorque(a);
    }
}
