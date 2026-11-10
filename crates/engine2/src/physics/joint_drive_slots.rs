//! `IPhysicsJoint`'s `Motor_*` (managed `ControlJoint`), `Wheel_*` and `Parallel_*` slots.

use super::joint::{update, with, with_sim};
use crate::math::{from_vec, to_vec};
use abi::slots::i_physics_joint as j;
use abi::{Registry, bind};

fn register_motor(r: &mut Registry) {
    bind!(r, j::MOTOR_SET_LINEAR_VELOCITY, |r, v| update(r, |s| s.motor.linear_velocity = to_vec(v)));
    bind!(r, j::MOTOR_GET_LINEAR_VELOCITY, |r| with(r, |s| from_vec(s.motor.linear_velocity)));
    bind!(r, j::MOTOR_SET_ANGULAR_VELOCITY, |r, v| update(r, |s| s.motor.angular_velocity = to_vec(v)));
    bind!(r, j::MOTOR_GET_ANGULAR_VELOCITY, |r| with(r, |s| from_vec(s.motor.angular_velocity)));
    bind!(r, j::MOTOR_SET_MAX_VELOCITY_FORCE, |r, v| update(r, |s| s.motor.max_velocity_force = v));
    bind!(r, j::MOTOR_GET_MAX_VELOCITY_FORCE, |r| with(r, |s| s.motor.max_velocity_force));
    bind!(r, j::MOTOR_SET_MAX_VELOCITY_TORQUE, |r, v| update(r, |s| s.motor.max_velocity_torque = v));
    bind!(r, j::MOTOR_GET_MAX_VELOCITY_TORQUE, |r| with(r, |s| s.motor.max_velocity_torque));
    bind!(r, j::MOTOR_SET_LINEAR_HERTZ, |r, v| update(r, |s| s.motor.linear_hertz = v));
    bind!(r, j::MOTOR_GET_LINEAR_HERTZ, |r| with(r, |s| s.motor.linear_hertz));
    bind!(r, j::MOTOR_SET_LINEAR_DAMPING_RATIO, |r, v| update(r, |s| s.motor.linear_damping = v));
    bind!(r, j::MOTOR_GET_LINEAR_DAMPING_RATIO, |r| with(r, |s| s.motor.linear_damping));
    bind!(r, j::MOTOR_SET_ANGULAR_HERTZ, |r, v| update(r, |s| s.motor.angular_hertz = v));
    bind!(r, j::MOTOR_GET_ANGULAR_HERTZ, |r| with(r, |s| s.motor.angular_hertz));
    bind!(r, j::MOTOR_SET_ANGULAR_DAMPING_RATIO, |r, v| update(r, |s| s.motor.angular_damping = v));
    bind!(r, j::MOTOR_GET_ANGULAR_DAMPING_RATIO, |r| with(r, |s| s.motor.angular_damping));
    bind!(r, j::MOTOR_SET_MAX_SPRING_FORCE, |r, v| update(r, |s| s.motor.max_spring_force = v));
    bind!(r, j::MOTOR_GET_MAX_SPRING_FORCE, |r| with(r, |s| s.motor.max_spring_force));
    bind!(r, j::MOTOR_SET_MAX_SPRING_TORQUE, |r, v| update(r, |s| s.motor.max_spring_torque = v));
    bind!(r, j::MOTOR_GET_MAX_SPRING_TORQUE, |r| with(r, |s| s.motor.max_spring_torque));
}

fn register_wheel(r: &mut Registry) {
    bind!(r, j::WHEEL_ENABLE_SUSPENSION, |r, v| update(r, |s| s.wheel.suspension = v));
    bind!(r, j::WHEEL_IS_SUSPENSION_ENABLED, |r| with(r, |s| s.wheel.suspension));
    bind!(r, j::WHEEL_SET_SUSPENSION_HERTZ, |r, v| update(r, |s| s.wheel.suspension_hertz = v));
    bind!(r, j::WHEEL_GET_SUSPENSION_HERTZ, |r| with(r, |s| s.wheel.suspension_hertz));
    bind!(r, j::WHEEL_SET_SUSPENSION_DAMPING_RATIO, |r, v| update(r, |s| s.wheel.suspension_damping = v));
    bind!(r, j::WHEEL_GET_SUSPENSION_DAMPING_RATIO, |r| with(r, |s| s.wheel.suspension_damping));
    bind!(r, j::WHEEL_ENABLE_SUSPENSION_LIMIT, |r, v| update(r, |s| s.wheel.suspension_limit = v));
    bind!(r, j::WHEEL_IS_SUSPENSION_LIMIT_ENABLED, |r| with(r, |s| s.wheel.suspension_limit));
    bind!(r, j::WHEEL_GET_LOWER_SUSPENSION_LIMIT, |r| with(r, |s| s.wheel.suspension_limits[0]));
    bind!(r, j::WHEEL_GET_UPPER_SUSPENSION_LIMIT, |r| with(r, |s| s.wheel.suspension_limits[1]));
    bind!(r, j::WHEEL_SET_SUSPENSION_LIMITS, |r, lo, hi| update(r, |s| s.wheel.suspension_limits = [lo, hi]));
    bind!(r, j::WHEEL_ENABLE_SPIN_MOTOR, |r, v| update(r, |s| s.wheel.spin_motor = v));
    bind!(r, j::WHEEL_IS_SPIN_MOTOR_ENABLED, |r| with(r, |s| s.wheel.spin_motor));
    bind!(r, j::WHEEL_SET_SPIN_MOTOR_SPEED, |r, v| update(r, |s| s.wheel.spin_speed = v));
    bind!(r, j::WHEEL_GET_SPIN_MOTOR_SPEED, |r| with(r, |s| s.wheel.spin_speed));
    bind!(r, j::WHEEL_SET_MAX_SPIN_TORQUE, |r, v| update(r, |s| s.wheel.max_spin_torque = v));
    bind!(r, j::WHEEL_GET_MAX_SPIN_TORQUE, |r| with(r, |s| s.wheel.max_spin_torque));
    bind!(r, j::WHEEL_ENABLE_STEERING, |r, v| update(r, |s| s.wheel.steering = v));
    bind!(r, j::WHEEL_IS_STEERING_ENABLED, |r| with(r, |s| s.wheel.steering));
    bind!(r, j::WHEEL_SET_STEERING_HERTZ, |r, v| update(r, |s| s.wheel.steering_hertz = v));
    bind!(r, j::WHEEL_GET_STEERING_HERTZ, |r| with(r, |s| s.wheel.steering_hertz));
    bind!(r, j::WHEEL_SET_STEERING_DAMPING_RATIO, |r, v| update(r, |s| s.wheel.steering_damping = v));
    bind!(r, j::WHEEL_GET_STEERING_DAMPING_RATIO, |r| with(r, |s| s.wheel.steering_damping));
    bind!(r, j::WHEEL_SET_MAX_STEERING_TORQUE, |r, v| update(r, |s| s.wheel.max_steering_torque = v));
    bind!(r, j::WHEEL_GET_MAX_STEERING_TORQUE, |r| with(r, |s| s.wheel.max_steering_torque));
    bind!(r, j::WHEEL_ENABLE_STEERING_LIMIT, |r, v| update(r, |s| s.wheel.steering_limit = v));
    bind!(r, j::WHEEL_IS_STEERING_LIMIT_ENABLED, |r| with(r, |s| s.wheel.steering_limit));
    bind!(r, j::WHEEL_GET_LOWER_STEERING_LIMIT, |r| with(r, |s| s.wheel.steering_limits[0]));
    bind!(r, j::WHEEL_GET_UPPER_STEERING_LIMIT, |r| with(r, |s| s.wheel.steering_limits[1]));
    bind!(r, j::WHEEL_SET_STEERING_LIMITS, |r, lo, hi| update(r, |s| s.wheel.steering_limits = [lo, hi]));
    bind!(r, j::WHEEL_SET_TARGET_STEERING_ANGLE, |r, v| update(r, |s| s.wheel.target_steering = v));
    bind!(r, j::WHEEL_GET_TARGET_STEERING_ANGLE, |r| with(r, |s| s.wheel.target_steering));
    // Spin is about frame X; steering is the twist about frame Z.
    bind!(r, j::WHEEL_GET_SPIN_SPEED, |r| with_sim(r, |sim, id| sim.joint_relative_spin(id).x));
    bind!(r, j::WHEEL_GET_STEERING_ANGLE, |r| with_sim(r, |sim, id| sim.joint_angle(id)));
    // TODO: per-axis motor torques; the joint's total angular impulse stands in for both.
    bind!(r, j::WHEEL_GET_SPIN_TORQUE, |r| with_sim(r, |sim, id| sim.joint_impulses(id).1));
    bind!(r, j::WHEEL_GET_STEERING_TORQUE, |r| with_sim(r, |sim, id| sim.joint_impulses(id).1));
}

pub fn register(r: &mut Registry) {
    register_motor(r);
    register_wheel(r);
    bind!(r, j::PARALLEL_SET_HERTZ, |r, v| update(r, |s| s.parallel.hertz = v));
    bind!(r, j::PARALLEL_GET_HERTZ, |r| with(r, |s| s.parallel.hertz));
    bind!(r, j::PARALLEL_SET_DAMPING_RATIO, |r, v| update(r, |s| s.parallel.damping = v));
    bind!(r, j::PARALLEL_GET_DAMPING_RATIO, |r| with(r, |s| s.parallel.damping));
    bind!(r, j::PARALLEL_SET_MAX_TORQUE, |r, v| update(r, |s| s.parallel.max_torque = v));
    bind!(r, j::PARALLEL_GET_MAX_TORQUE, |r| with(r, |s| s.parallel.max_torque));
}
