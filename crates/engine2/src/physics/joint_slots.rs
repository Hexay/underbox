//! `IPhysicsJoint`'s common slots (frames, limits, springs, motors, strength) and
//! `IPhysicsWorld.Add*Joint`/`RemoveJoint`. Motor, wheel and parallel joints are in
//! `joint_drive_slots.rs`.

use super::joint::{self, create, get, update, with, with_sim};
use super::{body, world};
use crate::math::{from_rot, from_vec, to_rot, to_vec};
use abi::slots::{i_physics_joint as j, i_physics_world as w};
use abi::{HandleId, Registry, bind};
use physics::{JointKind, Motor, Pose};

fn kind(r: joint::Ref) -> Option<JointKind> {
    with(r, |s| Some(s.kind))
}

/// Revolute joints keep min/max "length" as twist limits (radians), prismatic ones as linear
/// limits; setting either enables the limit (managed `HingeJoint.MinAngle`, `SliderJoint.MinLength`).
fn set_length(r: joint::Ref, end: usize, v: f32) {
    update(r, |s| match s.kind {
        JointKind::Revolute => (s.twist_limits[end], s.twist_limit_enabled) = (v, true),
        JointKind::Prismatic => (s.linear_limits[end], s.linear_limit_enabled) = (v, true),
        _ if end == 0 => s.min_length = v,
        _ => s.max_length = v,
    })
}

fn length(r: joint::Ref, end: usize) -> f32 {
    with(r, |s| match s.kind {
        JointKind::Revolute => s.twist_limits[end],
        JointKind::Prismatic => s.linear_limits[end],
        _ if end == 0 => s.min_length,
        _ => s.max_length,
    })
}

fn body_handle(b: body::Ref) -> HandleId<abi::class::IPhysicsBody> {
    body::get(b).map_or(HandleId::new(-1), |b| b.handle.id())
}

fn register_world(r: &mut Registry) {
    bind!(r, w::ADD_WELD_JOINT, |w, a, b, fa, fb| create(w, JointKind::Weld, a, b, fa, fb));
    bind!(r, w::ADD_SPRING_JOINT, |w, a, b, fa, fb| create(w, JointKind::Spring, a, b, fa, fb));
    bind!(r, w::ADD_REVOLUTE_JOINT, |w, a, b, fa, fb| create(w, JointKind::Revolute, a, b, fa, fb));
    bind!(r, w::ADD_PRISMATIC_JOINT, |w, a, b, fa, fb| create(w, JointKind::Prismatic, a, b, fa, fb));
    bind!(r, w::ADD_SPHERICAL_JOINT, |w, a, b, fa, fb| create(w, JointKind::Spherical, a, b, fa, fb));
    bind!(r, w::ADD_MOTOR_JOINT, |w, a, b, fa, fb| create(w, JointKind::Motor, a, b, fa, fb));
    bind!(r, w::ADD_WHEEL_JOINT, |w, a, b, fa, fb| create(w, JointKind::Wheel, a, b, fa, fb));
    bind!(r, w::ADD_PARALLEL_JOINT, |w, a, b, fa, fb| create(w, JointKind::Parallel, a, b, fa, fb));
    bind!(r, w::ADD_FILTER_JOINT, |w, a, b| create(w, JointKind::Filter, a, b, crate::math::identity(), crate::math::identity()));
    bind!(r, w::REMOVE_JOINT, |_, joint| joint::delete(joint));
}

pub fn register(r: &mut Registry) {
    register_world(r);
    bind!(r, j::GET_WORLD, |r| get(r).and_then(|j| world::get(j.world)).map_or(HandleId::new(-1), |w| w.handle.id()));
    bind!(r, j::GET_BODY1, |r| get(r).map_or(HandleId::new(-1), |j| body_handle(j.body1)));
    bind!(r, j::GET_BODY2, |r| get(r).map_or(HandleId::new(-1), |j| body_handle(j.body2)));
    bind!(r, j::GET_TYPE, |r| kind(r).map_or(0, |k| k as i64));
    bind!(r, j::GET_LOCAL_FRAME_A, |r, p, q| {
        let frame = with(r, |s| Some(s.frame1)).unwrap_or(Pose::IDENTITY);
        (*p, *q) = (from_vec(frame.translation), from_rot(frame.rotation));
    });
    bind!(r, j::GET_LOCAL_FRAME_B, |r, p, q| {
        let frame = with(r, |s| Some(s.frame2)).unwrap_or(Pose::IDENTITY);
        (*p, *q) = (from_vec(frame.translation), from_rot(frame.rotation));
    });
    bind!(r, j::SET_LOCAL_FRAME_A, |r, p, q| update(r, |s| s.frame1 = Pose::from_parts(to_vec(p), to_rot(q))));
    bind!(r, j::SET_LOCAL_FRAME_B, |r, p, q| update(r, |s| s.frame2 = Pose::from_parts(to_vec(p), to_rot(q))));
    bind!(r, j::SET_ENABLE_COLLISION, |r, v| update(r, |s| s.collide = v));
    bind!(r, j::IS_COLLISION_ENABLED, |r| with(r, |s| s.collide));

    bind!(r, j::SET_LINEAR_SPRING, |r, v| update(r, |s| {
        s.linear_spring = to_vec(v).to_array();
        s.linear_motor = None;
    }));
    bind!(r, j::GET_LINEAR_SPRING, |r| with(r, |s| from_vec(physics::Vector::from_array(s.linear_spring))));
    bind!(r, j::SET_ANGULAR_SPRING, |r, v| update(r, |s| {
        s.angular_spring = to_vec(v).to_array();
        s.angular_motor = None;
    }));
    bind!(r, j::GET_ANGULAR_SPRING, |r| with(r, |s| from_vec(physics::Vector::from_array(s.angular_spring))));
    bind!(r, j::SET_ANGULAR_MOTOR, |r, speed, max| update(r, |s| s.angular_motor = Some(Motor { speed, max })));
    bind!(r, j::SET_LINEAR_MOTOR, |r, speed, max| update(r, |s| s.linear_motor = Some(Motor { speed, max })));
    bind!(r, j::SET_FRICTION, |r, v| update(r, |s| {
        s.friction = v;
        (s.angular_motor, s.linear_motor, s.target_rotation, s.motor_velocity) = (None, None, None, None);
        if s.kind == JointKind::Revolute {
            s.angular_spring = [0.0; 3];
        } else if s.kind == JointKind::Prismatic {
            s.linear_spring = [0.0; 3];
        }
    }));
    bind!(r, j::SET_MIN_LENGTH, |r, v| set_length(r, 0, v));
    bind!(r, j::GET_MIN_LENGTH, |r| length(r, 0));
    bind!(r, j::SET_MAX_LENGTH, |r, v| set_length(r, 1, v));
    bind!(r, j::GET_MAX_LENGTH, |r| length(r, 1));
    bind!(r, j::SET_MIN_FORCE, |r, v| update(r, |s| s.min_force = v));
    bind!(r, j::GET_MIN_FORCE, |r| with(r, |s| s.min_force));
    bind!(r, j::SET_MAX_FORCE, |r, v| update(r, |s| s.max_force = v));
    bind!(r, j::GET_MAX_FORCE, |r| with(r, |s| s.max_force));

    bind!(r, j::SET_SWING_LIMIT, |r, degrees| update(r, |s| s.swing_limit = degrees.to_radians()));
    bind!(r, j::SET_SWING_LIMIT_ENABLED, |r, v| update(r, |s| s.swing_limit_enabled = v));
    bind!(r, j::SET_TWIST_LIMITS, |r, lo, hi| update(r, |s| s.twist_limits = [lo.to_radians(), hi.to_radians()]));
    bind!(r, j::SET_TWIST_LIMIT_ENABLED, |r, v| update(r, |s| s.twist_limit_enabled = v));
    bind!(r, j::SET_LINEAR_LIMITS, |r, lo, hi| update(r, |s| s.linear_limits = [lo, hi]));
    bind!(r, j::SET_LINEAR_LIMIT_ENABLED, |r, v| update(r, |s| s.linear_limit_enabled = v));
    bind!(r, j::SET_TARGET_ROTATION, |r, q, hertz, damping| update(r, |s| {
        s.target_rotation = Some((to_rot(q), hertz, damping));
        s.motor_velocity = None;
    }));
    bind!(r, j::SET_MOTOR_VELOCITY, |r, v, max| update(r, |s| {
        s.motor_velocity = Some((to_vec(v), max));
        s.target_rotation = None;
    }));

    bind!(r, j::GET_ANGLE, |r| with_sim(r, |sim, id| sim.joint_angle(id)));
    bind!(r, j::GET_LINEAR_IMPULSE, |r| with_sim(r, |sim, id| sim.joint_impulses(id).0));
    bind!(r, j::GET_ANGULAR_IMPULSE, |r| with_sim(r, |sim, id| sim.joint_impulses(id).1));
    bind!(r, j::GET_MAX_LINEAR_IMPULSE, |r| with(r, |s| s.max_linear_impulse));
    bind!(r, j::GET_MAX_ANGULAR_IMPULSE, |r| with(r, |s| s.max_angular_impulse));
    bind!(r, j::SET_MAX_LINEAR_IMPULSE, |r, v| update(r, |s| s.max_linear_impulse = v));
    bind!(r, j::SET_MAX_ANGULAR_IMPULSE, |r, v| update(r, |s| s.max_angular_impulse = v));
}
