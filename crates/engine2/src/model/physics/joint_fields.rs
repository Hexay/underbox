//! `VPhysXJoint_t`'s field accessors and limit getters/setters over [`JointDesc`]; the same
//! object type serves desc arrays (managed writes) and `CPhysicsData` (managed reads).

use super::data::JointDesc;
use super::desc::{JointRef, joint};
use abi::slots::v_phys_x_joint_t as j;
use abi::{Registry, bind};
use std::ffi::c_void;

fn with<R: Default>(p: *mut c_void, f: impl FnOnce(&mut JointDesc) -> R) -> R {
    joint(JointRef::from_raw(p)).map(|d| f(&mut d.lock().unwrap())).unwrap_or_default()
}

macro_rules! fields {
    ($r:expr, $($get:ident, $set:ident => $field:ident;)*) => {
        $(
            bind!($r, j::$get, |p| with(p, |d| d.$field));
            bind!($r, j::$set, |p, v| with(p, |d| d.$field = v));
        )*
    };
}

macro_rules! limits {
    ($r:expr, $($get:ident, $set:ident => $field:ident[$i:literal];)*) => {
        $(
            bind!($r, j::$get, |r: JointRef| with(r.as_raw(), |d| d.$field[$i]));
            bind!($r, j::$set, |r: JointRef, v| with(r.as_raw(), |d| d.$field[$i] = v));
        )*
    };
}

pub fn register(r: &mut Registry) {
    fields! { r,
        GET_M_N_TYPE, SET_M_N_TYPE => kind;
        GET_M_N_BODY1, SET_M_N_BODY1 => body1;
        GET_M_N_BODY2, SET_M_N_BODY2 => body2;
        GET_M_N_FLAGS, SET_M_N_FLAGS => flags;
        GET_M_B_ENABLE_COLLISION, SET_M_B_ENABLE_COLLISION => enable_collision;
        GET_M_B_ENABLE_LINEAR_LIMIT, SET_M_B_ENABLE_LINEAR_LIMIT => enable_linear_limit;
        GET_M_B_ENABLE_LINEAR_MOTOR, SET_M_B_ENABLE_LINEAR_MOTOR => enable_linear_motor;
        GET_M_V_LINEAR_TARGET_VELOCITY, SET_M_V_LINEAR_TARGET_VELOCITY => linear_target_velocity;
        GET_M_FL_MAX_FORCE, SET_M_FL_MAX_FORCE => max_force;
        GET_M_B_ENABLE_SWING_LIMIT, SET_M_B_ENABLE_SWING_LIMIT => enable_swing_limit;
        GET_M_B_ENABLE_TWIST_LIMIT, SET_M_B_ENABLE_TWIST_LIMIT => enable_twist_limit;
        GET_M_B_ENABLE_ANGULAR_MOTOR, SET_M_B_ENABLE_ANGULAR_MOTOR => enable_angular_motor;
        GET_M_V_ANGULAR_TARGET_VELOCITY, SET_M_V_ANGULAR_TARGET_VELOCITY => angular_target_velocity;
        GET_M_FL_MAX_TORQUE, SET_M_FL_MAX_TORQUE => max_torque;
        GET_M_FL_LINEAR_FREQUENCY, SET_M_FL_LINEAR_FREQUENCY => linear_frequency;
        GET_M_FL_LINEAR_DAMPING_RATIO, SET_M_FL_LINEAR_DAMPING_RATIO => linear_damping_ratio;
        GET_M_FL_ANGULAR_FREQUENCY, SET_M_FL_ANGULAR_FREQUENCY => angular_frequency;
        GET_M_FL_ANGULAR_DAMPING_RATIO, SET_M_FL_ANGULAR_DAMPING_RATIO => angular_damping_ratio;
        GET_M_FL_LINEAR_STRENGTH, SET_M_FL_LINEAR_STRENGTH => linear_strength;
        GET_M_FL_ANGULAR_STRENGTH, SET_M_FL_ANGULAR_STRENGTH => angular_strength;
        GET_M_B_LINEAR_MOTOR_IS_SPRING, SET_M_B_LINEAR_MOTOR_IS_SPRING => linear_motor_is_spring;
        GET_M_B_ANGULAR_MOTOR_IS_SPRING, SET_M_B_ANGULAR_MOTOR_IS_SPRING => angular_motor_is_spring;
        GET_M_FL_LINEAR_TARGET_POSITION, SET_M_FL_LINEAR_TARGET_POSITION => linear_target_position;
        GET_M_FL_ANGULAR_TARGET_ANGLE, SET_M_FL_ANGULAR_TARGET_ANGLE => angular_target_angle;
        GET_M_Q_ANGULAR_TARGET_ROTATION, SET_M_Q_ANGULAR_TARGET_ROTATION => angular_target_rotation;
        GET_M_B_OVERRIDE_FRICTION, SET_M_B_OVERRIDE_FRICTION => override_friction;
        GET_M_FL_FRICTION, SET_M_FL_FRICTION => friction;
        GET_M_FRAME1, SET_M_FRAME1 => frame1;
        GET_M_FRAME2, SET_M_FRAME2 => frame2;
    }
    limits! { r,
        GET_LINEAR_LIMIT_MIN, SET_LINEAR_LIMIT_MIN => linear_limit[0];
        GET_LINEAR_LIMIT_MAX, SET_LINEAR_LIMIT_MAX => linear_limit[1];
        GET_SWING_LIMIT_MIN, SET_SWING_LIMIT_MIN => swing_limit[0];
        GET_SWING_LIMIT_MAX, SET_SWING_LIMIT_MAX => swing_limit[1];
        GET_TWIST_LIMIT_MIN, SET_TWIST_LIMIT_MIN => twist_limit[0];
        GET_TWIST_LIMIT_MAX, SET_TWIST_LIMIT_MAX => twist_limit[1];
    }
}
