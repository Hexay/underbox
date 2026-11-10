//! `CPhysicsData`'s content: parts (one rigid body each, usually per bone) with their shapes, the
//! joints between parts, surfaces and collision attributes. Built from a `CPhysBodyDescArray`
//! ([`super::desc`]); read by managed's `PhysicsGroupDescription` and by aggregates.

use super::hull::Hull;
use abi::structs::{Rotation, Transform, Vector3};
use abi::{NativeRef, class};
use glam::Vec3;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Indices {
    pub surface: u32,
    pub attribute: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SphereDesc {
    pub center: Vec3,
    pub radius: f32,
    pub ids: Indices,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CapsuleDesc {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
    pub ids: Indices,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HullDesc {
    pub hull: Hull,
    pub ids: Indices,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshDesc {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Per-triangle surface-list index; empty when every triangle uses `ids.surface`.
    pub materials: Vec<u8>,
    pub ids: Indices,
}

/// The shapes of a part, in its body's space.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shapes {
    pub spheres: Vec<SphereDesc>,
    pub capsules: Vec<CapsuleDesc>,
    pub hulls: Vec<HullDesc>,
    pub meshes: Vec<MeshDesc>,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub bone: String,
    /// Model space.
    pub bind: Transform,
    pub flags: u32,
    pub mass: f32,
    pub inertia_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub gravity_scale: f32,
    pub override_mass_center: bool,
    pub mass_center: Vector3,
    pub collision_attribute: u16,
    /// The part's surface name token (`SetSurface`), if given.
    pub surface: Option<u32>,
    pub shapes: Shapes,
}

pub use crate::math::identity;

impl Default for Part {
    fn default() -> Self {
        Self {
            bone: String::new(),
            bind: identity(),
            flags: 0,
            mass: 0.0,
            inertia_scale: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            override_mass_center: false,
            mass_center: Vector3::default(),
            collision_attribute: 0,
            surface: None,
            shapes: Shapes::default(),
        }
    }
}

/// `VPhysXJoint_t`: what managed writes through the joint's field accessors.
#[derive(Debug, Clone, Copy)]
pub struct JointDesc {
    pub kind: u16,
    pub body1: u16,
    pub body2: u16,
    pub flags: u16,
    pub enable_collision: bool,
    pub enable_linear_limit: bool,
    pub enable_linear_motor: bool,
    pub linear_target_velocity: Vector3,
    pub max_force: f32,
    pub enable_swing_limit: bool,
    pub enable_twist_limit: bool,
    pub enable_angular_motor: bool,
    pub angular_target_velocity: Vector3,
    pub max_torque: f32,
    pub linear_frequency: f32,
    pub linear_damping_ratio: f32,
    pub angular_frequency: f32,
    pub angular_damping_ratio: f32,
    pub linear_strength: f32,
    pub angular_strength: f32,
    pub linear_motor_is_spring: bool,
    pub angular_motor_is_spring: bool,
    pub linear_target_position: f32,
    pub angular_target_angle: f32,
    pub angular_target_rotation: Rotation,
    pub override_friction: bool,
    pub friction: f32,
    /// Each body's joint frame, local to that body.
    pub frame1: Transform,
    pub frame2: Transform,
    /// (min, max); angles in radians.
    pub linear_limit: [f32; 2],
    pub swing_limit: [f32; 2],
    pub twist_limit: [f32; 2],
}

impl Default for JointDesc {
    fn default() -> Self {
        Self {
            kind: 0,
            body1: 0,
            body2: 0,
            flags: 0,
            enable_collision: false,
            enable_linear_limit: false,
            enable_linear_motor: false,
            linear_target_velocity: Vector3::default(),
            max_force: 0.0,
            enable_swing_limit: false,
            enable_twist_limit: false,
            enable_angular_motor: false,
            angular_target_velocity: Vector3::default(),
            max_torque: 0.0,
            linear_frequency: 0.0,
            linear_damping_ratio: 0.0,
            angular_frequency: 0.0,
            angular_damping_ratio: 0.0,
            linear_strength: 0.0,
            angular_strength: 0.0,
            linear_motor_is_spring: false,
            angular_motor_is_spring: false,
            linear_target_position: 0.0,
            angular_target_angle: 0.0,
            angular_target_rotation: identity().rotation,
            override_friction: false,
            friction: 0.0,
            frame1: identity(),
            frame2: identity(),
            linear_limit: [0.0; 2],
            swing_limit: [0.0; 2],
            twist_limit: [0.0; 2],
        }
    }
}

pub type SharedJoint = Arc<Mutex<JointDesc>>;

pub struct PhysicsData {
    pub name: String,
    pub parts: Vec<Arc<Part>>,
    pub joints: Vec<SharedJoint>,
    /// Global surface indices (managed `Surface.Index`) shapes' surface indices point into.
    pub surfaces: Vec<i32>,
    /// Each collision attribute's tags.
    pub attributes: Vec<Vec<String>>,
    pub(super) part_refs: OnceLock<Vec<NativeRef<class::VPhysXBodyPart_t>>>,
    pub(super) joint_refs: OnceLock<Vec<NativeRef<class::VPhysXJoint_t>>>,
}

impl PhysicsData {
    pub fn new(name: &str, parts: Vec<Part>, joints: Vec<JointDesc>, surfaces: Vec<i32>) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_owned(),
            parts: parts.into_iter().map(Arc::new).collect(),
            joints: joints.into_iter().map(|j| Arc::new(Mutex::new(j))).collect(),
            surfaces,
            // Runtime parts all use the one attribute every shape points at.
            attributes: vec![vec!["solid".to_owned()]],
            part_refs: OnceLock::new(),
            joint_refs: OnceLock::new(),
        })
    }

    pub fn joint(&self, i: usize) -> Option<JointDesc> {
        self.joints.get(i).map(|j| *j.lock().unwrap())
    }

    /// Bones only count when some part names one (PhysicsGroupDescription reads poses then).
    pub fn has_bones(&self) -> bool {
        self.parts.iter().any(|p| !p.bone.is_empty())
    }
}

impl Drop for PhysicsData {
    fn drop(&mut self) {
        self.part_refs.get().into_iter().flatten().for_each(|r| {
            objects::destroy(*r);
        });
        self.joint_refs.get().into_iter().flatten().for_each(|r| {
            objects::destroy(*r);
        });
    }
}
