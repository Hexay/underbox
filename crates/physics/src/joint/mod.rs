//! Joints as managed's `IPhysicsJoint` describes them (Box3D-style: local frames, hertz/damping
//! springs, motors, limits, break strengths), kept as plain settings and rebuilt into a Rapier
//! joint on every change. Axis conventions are in `build.rs`.

mod build;
#[cfg(test)]
mod tests;

use crate::world::World;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ImpulseJointHandle, RigidBodyHandle};
use std::collections::HashMap;

pub use rapier3d::prelude::ImpulseJointHandle as JointId;

/// managed's `PhysicsJointType` values for the joints `IPhysicsWorld.Add*Joint` makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointKind {
    /// Collision filter only.
    Filter = 0,
    Spherical = 1,
    Prismatic = 2,
    Revolute = 3,
    Weld = 6,
    Spring = 7,
    /// Box3D's motor joint (managed `ControlJoint`).
    Motor = 13,
    Parallel = 15,
    Wheel = 16,
}

/// (hertz, damping ratio, third), where the third value depends on the joint: max force (weld),
/// rest length (spring), target position (prismatic); revolute's angular spring is
/// (target angle, hertz, damping ratio) instead (managed `HingeJoint.ApplyMotor`).
pub type Spring = [f32; 3];

/// A motor: target speed and the most force or torque it may use.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Motor {
    pub speed: f32,
    pub max: f32,
}

/// Box3D motor joint settings (managed `ControlJoint`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MotorJoint {
    pub linear_velocity: Vector,
    pub angular_velocity: Vector,
    pub max_velocity_force: f32,
    pub max_velocity_torque: f32,
    pub linear_hertz: f32,
    pub linear_damping: f32,
    pub angular_hertz: f32,
    pub angular_damping: f32,
    pub max_spring_force: f32,
    pub max_spring_torque: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Wheel {
    pub suspension: bool,
    pub suspension_hertz: f32,
    pub suspension_damping: f32,
    pub suspension_limit: bool,
    pub suspension_limits: [f32; 2],
    pub spin_motor: bool,
    pub spin_speed: f32,
    pub max_spin_torque: f32,
    pub steering: bool,
    pub steering_hertz: f32,
    pub steering_damping: f32,
    pub max_steering_torque: f32,
    pub steering_limit: bool,
    pub steering_limits: [f32; 2],
    pub target_steering: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Parallel {
    pub hertz: f32,
    pub damping: f32,
    pub max_torque: f32,
}

/// Everything managed can set on a joint. Angles are radians.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    pub kind: JointKind,
    pub frame1: Pose,
    pub frame2: Pose,
    pub collide: bool,
    pub linear_spring: Spring,
    pub angular_spring: Spring,
    pub min_length: f32,
    pub max_length: f32,
    pub min_force: f32,
    pub max_force: f32,
    pub friction: f32,
    pub swing_limit: f32,
    pub swing_limit_enabled: bool,
    pub twist_limits: [f32; 2],
    pub twist_limit_enabled: bool,
    pub linear_limits: [f32; 2],
    pub linear_limit_enabled: bool,
    pub angular_motor: Option<Motor>,
    pub linear_motor: Option<Motor>,
    /// Spherical: drive to a rotation (rotation, hertz, damping ratio).
    pub target_rotation: Option<(Rotation, f32, f32)>,
    /// Spherical: drive at an angular velocity, with a max torque.
    pub motor_velocity: Option<(Vector, f32)>,
    /// Break strengths; zero or less never breaks.
    pub max_linear_impulse: f32,
    pub max_angular_impulse: f32,
    pub motor: MotorJoint,
    pub wheel: Wheel,
    pub parallel: Parallel,
}

impl Joint {
    pub fn new(kind: JointKind, frame1: Pose, frame2: Pose) -> Self {
        Self {
            kind,
            frame1,
            frame2,
            collide: false,
            linear_spring: [0.0; 3],
            angular_spring: [0.0; 3],
            min_length: 0.0,
            max_length: f32::MAX,
            min_force: f32::MIN,
            max_force: f32::MAX,
            friction: 0.0,
            swing_limit: 0.0,
            swing_limit_enabled: false,
            twist_limits: [0.0; 2],
            twist_limit_enabled: false,
            linear_limits: [0.0; 2],
            linear_limit_enabled: false,
            angular_motor: None,
            linear_motor: None,
            target_rotation: None,
            motor_velocity: None,
            max_linear_impulse: 0.0,
            max_angular_impulse: 0.0,
            motor: MotorJoint::default(),
            wheel: Wheel::default(),
            parallel: Parallel::default(),
        }
    }
}

/// A joint's settings plus what the last step measured.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub joint: Joint,
    pub linear_impulse: f32,
    pub angular_impulse: f32,
}

#[derive(Default)]
pub(crate) struct Joints {
    pub entries: HashMap<ImpulseJointHandle, Entry>,
    /// Joints that broke since the last `take_broken_joints`.
    pub broken: Vec<(ImpulseJointHandle, u128)>,
}

impl World {
    pub fn add_joint(&mut self, a: RigidBodyHandle, b: RigidBodyHandle, joint: Joint) -> Option<JointId> {
        self.body(a)?;
        self.body(b)?;
        let data = build::rapier(&joint, self.bodies_pair(a, b));
        let id = self.raw_mut().insert_impulse_joint(a, b, data);
        self.joints.entries.insert(id, Entry { joint, linear_impulse: 0.0, angular_impulse: 0.0 });
        Some(id)
    }

    pub fn remove_joint(&mut self, id: JointId) {
        self.joints.entries.remove(&id);
        self.raw_mut().remove_impulse_joint(id);
    }

    pub fn joint(&self, id: JointId) -> Option<&Joint> {
        self.joints.entries.get(&id).map(|e| &e.joint)
    }

    /// The joint's bodies, (first, second).
    pub fn joint_bodies(&self, id: JointId) -> Option<(RigidBodyHandle, RigidBodyHandle)> {
        self.raw().impulse_joints.get(id).map(|j| (j.body1(), j.body2()))
    }

    /// Change a joint's settings; the Rapier joint is rebuilt from them and its bodies woken.
    pub fn update_joint(&mut self, id: JointId, f: impl FnOnce(&mut Joint)) {
        let Some(entry) = self.joints.entries.get_mut(&id) else { return };
        f(&mut entry.joint);
        let joint = entry.joint.clone();
        let Some((a, b)) = self.joint_bodies(id) else { return };
        let data = build::rapier(&joint, self.bodies_pair(a, b));
        if let Some(j) = self.raw_mut().impulse_joints.get_mut(id, true) {
            let user_data = j.data.user_data;
            j.data = data;
            j.data.user_data = user_data;
        }
    }

    pub fn set_joint_user_data(&mut self, id: JointId, data: u128) {
        if let Some(j) = self.raw_mut().impulse_joints.get_mut(id, false) {
            j.data.user_data = data;
        }
    }

    /// (linear, angular) impulse the joint applied over the last step.
    pub fn joint_impulses(&self, id: JointId) -> (f32, f32) {
        self.joints.entries.get(&id).map_or((0.0, 0.0), |e| (e.linear_impulse, e.angular_impulse))
    }

    /// Hinge angle, or twist for other joints: frame2's rotation about frame1's axis.
    pub fn joint_angle(&self, id: JointId) -> f32 {
        let (Some(joint), Some((a, b))) = (self.joint(id), self.joint_bodies(id)) else { return 0.0 };
        let (Some(ra), Some(rb)) = (self.body(a), self.body(b)) else { return 0.0 };
        build::twist(&(*ra.position() * joint.frame1), &(*rb.position() * joint.frame2))
    }

    /// Body 2's angular velocity relative to body 1's, in joint frame 1's axes.
    pub fn joint_relative_spin(&self, id: JointId) -> Vector {
        let (Some(joint), Some((a, b))) = (self.joint(id), self.joint_bodies(id)) else { return Vector::ZERO };
        let (Some(ra), Some(rb)) = (self.body(a), self.body(b)) else { return Vector::ZERO };
        let frame = ra.rotation() * joint.frame1.rotation;
        frame.inverse() * (rb.angvel() - ra.angvel())
    }

    /// Joints that broke since the last call, with their user data; they are already removed.
    pub fn take_broken_joints(&mut self) -> Vec<(JointId, u128)> {
        std::mem::take(&mut self.joints.broken)
    }

    fn bodies_pair(&self, a: RigidBodyHandle, b: RigidBodyHandle) -> build::Bodies {
        let pose = |h| self.body(h).map_or(Pose::IDENTITY, |rb| *rb.position());
        build::Bodies { a: pose(a), b: pose(b) }
    }

    /// Before a step: re-aim springs that only push or only pull.
    pub(crate) fn drive_joints(&mut self) {
        let springs: Vec<_> = self.joints.entries.iter().filter(|(_, e)| e.joint.kind == JointKind::Spring).map(|(h, _)| *h).collect();
        for id in springs {
            let Some(joint) = self.joint(id).cloned() else { continue };
            if joint.min_force >= 0.0 || joint.max_force <= 0.0 {
                let Some((a, b)) = self.joint_bodies(id) else { continue };
                let data = build::rapier(&joint, self.bodies_pair(a, b));
                if let Some(j) = self.raw_mut().impulse_joints.get_mut(id, false) {
                    j.data.motors = data.motors;
                    j.data.motor_axes = data.motor_axes;
                }
            }
        }
    }

    /// After a substep of `dt`: record joint impulses and break any over its strength.
    pub(crate) fn measure_joints(&mut self, substeps: u32) {
        let mut broken = Vec::new();
        for (h, j) in self.inner.impulse_joints.iter() {
            let Some(entry) = self.joints.entries.get_mut(&h) else { continue };
            let i = j.impulses;
            let linear = Vector::new(i[0], i[1], i[2]).length() * substeps as f32;
            let angular = Vector::new(i[3], i[4], i[5]).length() * substeps as f32;
            (entry.linear_impulse, entry.angular_impulse) = (linear, angular);
            let over = |max: f32, v: f32| max > 0.0 && v > max;
            if over(entry.joint.max_linear_impulse, linear) || over(entry.joint.max_angular_impulse, angular) {
                broken.push((h, j.data.user_data));
            }
        }
        for (h, data) in broken {
            self.remove_joint(h);
            self.joints.broken.push((h, data));
        }
    }
}
