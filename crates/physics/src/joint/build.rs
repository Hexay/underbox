//! [`Joint`] settings → a Rapier [`GenericJoint`].
//!
//! Axes: managed (Box3D) joints act along or about their frames' Z (hinge axis, slide axis,
//! twist axis, wheel suspension and steering); a wheel spins about frame X. Rapier's revolute,
//! prismatic and twist axis is X, so each frame is post-rotated by [`axis_frame`] (Rapier X →
//! managed Z, Y → Y, Z → −X). In Rapier's terms: LinX/AngX = managed Z, AngZ = managed −X.
//! Springs are acceleration-based, so hertz and damping ratio mean what they do in Box3D.

use super::{Joint, JointKind, Motor};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{GenericJoint, JointAxesMask, JointAxis, MotorModel, SpringCoefficients};
use std::f32::consts::TAU;

/// World poses of the joint's two bodies (for springs that only push or only pull).
pub(crate) struct Bodies {
    pub a: Pose,
    pub b: Pose,
}

/// Damping for velocity motors: high enough to track the target speed, within their max force.
const VELOCITY_GAIN: f32 = 1.0e4;

/// Softness of the locked axes. Real's joints give way to contacts between their own bodies:
/// boxes overlapping by 4 rest 0.8 further apart, wheels sunk into a chassis lift it 0.75
/// (`joint/soft_tests.rs`). Rapier's default (1 MHz) holds them rigid.
const LOCK_HERTZ: f32 = 80.0;
const LOCK_DAMPING: f32 = 40.0;

fn axis_frame() -> Rotation {
    Rotation::from_rotation_arc(Vector::X, Vector::Z)
}

const LIN: [JointAxis; 3] = [JointAxis::LinX, JointAxis::LinY, JointAxis::LinZ];
const ANG: [JointAxis; 3] = [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ];

/// A scene step's substep: 0.02 s over `max(2, 4)` integrations (`world.rs`).
const SUBSTEP: f32 = 0.005;

/// Real solves a spring twice per substep (again after integrating positions), Rapier once. Extra
/// damping of `SUBSTEP·ω/2` reproduces that within 1% up to 2 Hz and 10% at 5 Hz (probe `HINGE
/// spring`); above about 10 Hz it overshoots, so stiffer springs keep their own damping.
const RELAX_DAMPING_MAX_HERTZ: f32 = 10.0;

/// (stiffness, damping) for a spring of `hertz` and damping ratio `zeta`.
fn spring(hertz: f32, zeta: f32) -> (f32, f32) {
    let omega = TAU * hertz;
    let zeta = if hertz <= RELAX_DAMPING_MAX_HERTZ { zeta + SUBSTEP * omega / 2.0 } else { zeta };
    (omega * omega, 2.0 * zeta * omega)
}

fn drive(g: &mut GenericJoint, axis: JointAxis, pos: f32, vel: f32, (stiffness, damping): (f32, f32), max: f32) {
    g.set_motor(axis, pos, vel, stiffness, damping).set_motor_model(axis, MotorModel::AccelerationBased).set_motor_max_force(axis, max.max(0.0));
}

fn velocity_motor(g: &mut GenericJoint, axis: JointAxis, m: Motor) {
    drive(g, axis, 0.0, m.speed, (0.0, VELOCITY_GAIN), m.max);
}

/// A spring towards `target` on `axis`, or nothing at zero hertz.
fn spring_to(g: &mut GenericJoint, axis: JointAxis, target: f32, hertz: f32, zeta: f32, max: f32) -> bool {
    if hertz <= 0.0 {
        return false;
    }
    drive(g, axis, target, 0.0, spring(hertz, zeta), max);
    true
}

/// Managed-frame vector → Rapier-frame vector.
fn to_rapier(v: Vector) -> Vector {
    axis_frame().inverse() * v
}

pub(crate) fn rapier(j: &Joint, bodies: Bodies) -> GenericJoint {
    let mut g = GenericJoint::default();
    let r = axis_frame();
    g.set_local_frame1(Pose::from_parts(j.frame1.translation, j.frame1.rotation * r));
    g.set_local_frame2(Pose::from_parts(j.frame2.translation, j.frame2.rotation * r));
    g.set_contacts_enabled(j.collide);
    g.softness = SpringCoefficients::new(LOCK_HERTZ, LOCK_DAMPING);
    let friction = Motor { speed: 0.0, max: j.friction };
    match j.kind {
        JointKind::Filter => {}
        JointKind::Weld => {
            let [lh, lz, lmax] = j.linear_spring;
            let [ah, az, amax] = j.angular_spring;
            for axis in LIN {
                if !spring_to(&mut g, axis, 0.0, lh, lz, lmax) {
                    g.lock_axes(axis.into());
                }
            }
            for axis in ANG {
                if !spring_to(&mut g, axis, 0.0, ah, az, amax) {
                    g.lock_axes(axis.into());
                }
            }
        }
        JointKind::Revolute => {
            g.lock_axes(JointAxesMask::LOCKED_REVOLUTE_AXES);
            if j.twist_limit_enabled {
                g.set_limits(JointAxis::AngX, j.twist_limits);
            }
            // Real ignores hinge friction, up to 1e7 (probe `HINGE friction`); setting it only
            // clears the motor and spring (`joint_slots.rs`).
            let [target, hertz, zeta] = j.angular_spring;
            if let Some(m) = j.angular_motor {
                velocity_motor(&mut g, JointAxis::AngX, m);
            } else {
                spring_to(&mut g, JointAxis::AngX, target, hertz, zeta, f32::MAX);
            }
        }
        JointKind::Prismatic => {
            g.lock_axes(JointAxesMask::LOCKED_PRISMATIC_AXES);
            if j.linear_limit_enabled {
                g.set_limits(JointAxis::LinX, j.linear_limits);
            }
            let [hertz, zeta, target] = j.linear_spring;
            if let Some(m) = j.linear_motor {
                velocity_motor(&mut g, JointAxis::LinX, m);
            } else if !spring_to(&mut g, JointAxis::LinX, target, hertz, zeta, f32::MAX) && j.friction > 0.0 {
                velocity_motor(&mut g, JointAxis::LinX, friction);
            }
        }
        JointKind::Spherical => spherical(&mut g, j, friction),
        JointKind::Spring => spring_joint(&mut g, j, bodies),
        JointKind::Motor => motor_joint(&mut g, j),
        JointKind::Parallel => {
            let p = j.parallel;
            spring_to(&mut g, JointAxis::AngY, 0.0, p.hertz, p.damping, p.max_torque);
            spring_to(&mut g, JointAxis::AngZ, 0.0, p.hertz, p.damping, p.max_torque);
        }
        JointKind::Wheel => wheel(&mut g, j),
    }
    g
}

fn spherical(g: &mut GenericJoint, j: &Joint, friction: Motor) {
    g.lock_axes(JointAxesMask::LIN_AXES);
    if j.twist_limit_enabled {
        g.set_limits(JointAxis::AngX, j.twist_limits);
    }
    if j.swing_limit_enabled {
        g.set_limits(JointAxis::AngY, [-j.swing_limit, j.swing_limit]);
        g.set_limits(JointAxis::AngZ, [-j.swing_limit, j.swing_limit]);
    }
    if let Some((rotation, hertz, zeta)) = j.target_rotation {
        let target = to_rapier(rotation.to_scaled_axis());
        for (axis, t) in ANG.into_iter().zip(target.to_array()) {
            spring_to(g, axis, t, hertz, zeta, f32::MAX);
        }
    } else if let Some((velocity, max)) = j.motor_velocity {
        for (axis, v) in ANG.into_iter().zip(to_rapier(velocity).to_array()) {
            velocity_motor(g, axis, Motor { speed: v, max });
        }
    } else if j.friction > 0.0 {
        ANG.into_iter().for_each(|axis| velocity_motor(g, axis, friction));
    }
}

/// A distance spring between the frames' origins: rest length in `linear_spring[2]`, kept within
/// [min_length, max_length]; `min_force >= 0` only pushes, `max_force <= 0` only pulls.
fn spring_joint(g: &mut GenericJoint, j: &Joint, bodies: Bodies) {
    g.set_local_frame1(Pose::from_translation(j.frame1.translation));
    g.set_local_frame2(Pose::from_translation(j.frame2.translation));
    g.coupled_axes = JointAxesMask::LIN_AXES;
    if j.min_length > 0.0 || j.max_length < f32::MAX {
        g.set_limits(JointAxis::LinX, [j.min_length.max(0.0), j.max_length.max(j.min_length)]);
    }
    let [hertz, zeta, rest] = j.linear_spring;
    let length = ((bodies.b * j.frame2.translation) - (bodies.a * j.frame1.translation)).length();
    let push_only = j.min_force >= 0.0;
    let pull_only = j.max_force <= 0.0;
    let active = !(push_only && length > rest) && !(pull_only && length < rest);
    if active {
        let max = j.max_force.abs().max(j.min_force.abs());
        spring_to(g, JointAxis::LinX, rest, hertz, zeta, max);
    }
}

fn motor_joint(g: &mut GenericJoint, j: &Joint) {
    let m = j.motor;
    let gains = |hertz: f32, zeta: f32, max_velocity: f32| {
        let (k, c) = spring(hertz, zeta);
        (k, if max_velocity > 0.0 { VELOCITY_GAIN } else { c })
    };
    let lin = to_rapier(m.linear_velocity).to_array();
    let ang = to_rapier(m.angular_velocity).to_array();
    for (axis, v) in LIN.into_iter().zip(lin) {
        drive(g, axis, 0.0, v, gains(m.linear_hertz, m.linear_damping, m.max_velocity_force), m.max_velocity_force + m.max_spring_force);
    }
    for (axis, v) in ANG.into_iter().zip(ang) {
        drive(g, axis, 0.0, v, gains(m.angular_hertz, m.angular_damping, m.max_velocity_torque), m.max_velocity_torque + m.max_spring_torque);
    }
}

fn wheel(g: &mut GenericJoint, j: &Joint) {
    let w = j.wheel;
    g.lock_axes(JointAxis::LinY.into());
    g.lock_axes(JointAxis::LinZ.into());
    g.lock_axes(JointAxis::AngY.into());
    if !w.suspension || !spring_to(g, JointAxis::LinX, 0.0, w.suspension_hertz, w.suspension_damping, f32::MAX) {
        g.lock_axes(JointAxis::LinX.into());
    }
    if w.suspension_limit {
        g.set_limits(JointAxis::LinX, w.suspension_limits);
    }
    if w.spin_motor {
        velocity_motor(g, JointAxis::AngZ, Motor { speed: -w.spin_speed, max: w.max_spin_torque });
    }
    if !w.steering {
        g.lock_axes(JointAxis::AngX.into());
        return;
    }
    spring_to(g, JointAxis::AngX, w.target_steering, w.steering_hertz, w.steering_damping, w.max_steering_torque);
    if w.steering_limit {
        g.set_limits(JointAxis::AngX, w.steering_limits);
    }
}

/// Rotation of world frame `b` relative to world frame `a` about `a`'s Z, in (−π, π].
pub(crate) fn twist(a: &Pose, b: &Pose) -> f32 {
    let q = a.rotation.inverse() * b.rotation;
    let angle = 2.0 * q.z.atan2(q.w);
    if angle > std::f32::consts::PI { angle - TAU } else if angle <= -std::f32::consts::PI { angle + TAU } else { angle }
}
