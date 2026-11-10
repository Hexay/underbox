//! `IPhysAggregateInstance` (managed `PhysicsGroup`): a model's physics made real in a world, one
//! body per part (with its shapes) and a joint per described joint. Created by
//! `IPhysicsWorld::CreateAggregateInstance` from a model or a `CPhysicsData` resource.

use super::{body, joint, shape, world};
use crate::handles::{Handle, Kind};
use crate::math::{quat, v3};
use crate::model::physics::data::{JointDesc, Part, PhysicsData};
use abi::structs::{PhysicsBodySettings, PhysicsShapeSettings, Transform};
use abi::{HandleId, NativeRef, StringToken, class};
use glam::Vec3;
use physics::{BodyType, Joint, JointKind, Pose, Shape};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

pub type Ref = NativeRef<class::IPhysAggregateInstance>;

pub struct Aggregate {
    pub world: world::Ref,
    pub handle: Handle,
    pub data: Arc<PhysicsData>,
    /// Each part's body, with the part's bone name.
    pub bodies: Vec<(String, body::Ref)>,
    pub joints: Mutex<Vec<joint::Ref>>,
}

/// Which aggregate a body belongs to (`IPhysicsBody::GetAggregate`).
static OWNERS: LazyLock<Mutex<HashMap<usize, Ref>>> = LazyLock::new(Default::default);

pub fn get(r: Ref) -> Option<Arc<Aggregate>> {
    objects::get::<_, Aggregate>(r)
}

pub fn of_body(b: body::Ref) -> Option<Ref> {
    OWNERS.lock().unwrap().get(&b.addr()).copied()
}

/// managed's `PhysicsMotionType`: Invalid, Dynamic, Static, Keyframed.
fn body_type(motion: i64) -> BodyType {
    match motion {
        2 => BodyType::Static,
        3 => BodyType::Keyframed,
        _ => BodyType::Dynamic,
    }
}

/// managed's `PhysicsBodyCreationFlags`: gravity, auto sleep, awake, (bullet), override centre.
fn body_settings(p: &Part) -> PhysicsBodySettings {
    let flags = 1 | 2 | 4 | if p.override_mass_center { 1 << 4 } else { 0 };
    PhysicsBodySettings {
        mass: p.mass,
        inertia_scale: if p.inertia_scale > 0.0 { p.inertia_scale } else { 1.0 },
        linear_damping: p.linear_damping,
        angular_damping: p.angular_damping,
        gravity_scale: p.gravity_scale,
        mass_center: p.mass_center,
        flags,
        ..Default::default()
    }
}

fn shape_settings(data: &PhysicsData, p: &Part) -> PhysicsShapeSettings {
    let mut tags = [0u32; 16];
    let attribute = data.attributes.get(usize::from(p.collision_attribute)).cloned().unwrap_or_default();
    for (slot, tag) in tags.iter_mut().zip(attribute) {
        *slot = StringToken::of(&tag).0;
    }
    PhysicsShapeSettings { tags, friction: -1.0, elasticity: -1.0, surface_index: -1, collision_function_mask: u32::from(physics::function::DEFAULT), ..Default::default() }
}

fn add_shapes(b: body::Ref, data: &PhysicsData, p: &Part) {
    let s = &p.shapes;
    let add = |kind, geometry| {
        shape::add(b, kind, geometry, shape_settings(data, p));
    };
    let pv = |v: Vec3| physics::Vector::new(v.x, v.y, v.z);
    s.spheres.iter().for_each(|x| add(shape::Type::Sphere, Shape::Sphere { center: pv(x.center), radius: x.radius }));
    s.capsules.iter().for_each(|x| add(shape::Type::Capsule, Shape::Capsule { a: pv(x.a), b: pv(x.b), radius: x.radius }));
    s.hulls.iter().for_each(|x| add(shape::Type::Hull, Shape::Hull { pose: Pose::IDENTITY, points: x.hull.vertices.iter().copied().map(pv).collect() }));
    s.meshes.iter().for_each(|x| add(shape::Type::Mesh, Shape::Mesh { vertices: x.vertices.iter().copied().map(pv).collect(), triangles: x.triangles.clone() }));
}

fn pose(t: Transform) -> Pose {
    Pose::from_parts(crate::math::to_vec(t.position), crate::math::to_rot(t.rotation))
}

/// managed's `PhysicsJointType` → ours; joint frames twist about X (PhysX), ours about Z.
fn joint_settings(j: &JointDesc) -> Option<Joint> {
    let kind = match j.kind {
        1 | 4 | 10 => JointKind::Spherical,
        2 => JointKind::Prismatic,
        3 => JointKind::Revolute,
        6 => JointKind::Weld,
        7 => JointKind::Spring,
        _ => return None,
    };
    let x_to_z = |t: Transform| {
        let p = pose(t);
        Pose::from_parts(p.translation, p.rotation * physics::Rotation::from_rotation_arc(physics::Vector::Z, physics::Vector::X))
    };
    let mut s = Joint::new(kind, x_to_z(j.frame1), x_to_z(j.frame2));
    s.collide = j.enable_collision;
    s.twist_limit_enabled = j.enable_twist_limit;
    s.twist_limits = j.twist_limit;
    s.swing_limit_enabled = j.enable_swing_limit;
    s.swing_limit = j.swing_limit[1].abs().max(j.swing_limit[0].abs());
    s.linear_limit_enabled = j.enable_linear_limit;
    s.linear_limits = j.linear_limit;
    if j.override_friction {
        s.friction = j.friction;
    }
    Some(s)
}

pub fn create(world_ref: world::Ref, data: Arc<PhysicsData>, start: Transform, motion: i64) -> HandleId<class::IPhysAggregateInstance> {
    spawn(world_ref, data, start, motion).and_then(get).map_or(HandleId::new(-1), |a| a.handle.id())
}

/// Bodies at `start` × each part's bind pose, then the joints; the aggregate itself.
pub fn spawn(world_ref: world::Ref, data: Arc<PhysicsData>, start: Transform, motion: i64) -> Option<Ref> {
    world::get(world_ref)?;
    let (origin, turn) = (v3(start.position), quat(start.rotation));
    let mut bodies = Vec::with_capacity(data.parts.len());
    for p in &data.parts {
        let (at, rot) = (origin + turn * v3(p.bind.position), (turn * quat(p.bind.rotation)).normalize());
        let pose = Pose::from_parts(physics::Vector::new(at.x, at.y, at.z), physics::Rotation::from_xyzw(rot.x, rot.y, rot.z, rot.w));
        let Some(b) = body::spawn(world_ref, body_type(motion), pose, &body_settings(p)) else { continue };
        add_shapes(b, &data, p);
        bodies.push((p.bone.clone(), b));
    }
    let mut joints = Vec::new();
    for j in (0..data.joints.len()).filter_map(|i| data.joint(i)) {
        let (Some(a), Some(b)) = (bodies.get(usize::from(j.body1)), bodies.get(usize::from(j.body2))) else { continue };
        if let Some(r) = joint_settings(&j).and_then(|s| joint::spawn(world_ref, s, a.1, b.1)) {
            joints.push(r);
        }
    }
    let aggregate = Arc::new(Aggregate { world: world_ref, handle: Handle::default(), data, bodies, joints: Mutex::new(joints) });
    let r: Ref = objects::create_shared(aggregate.clone());
    OWNERS.lock().unwrap().extend(aggregate.bodies.iter().map(|(_, b)| (b.addr(), r)));
    aggregate.handle.register::<_, class::IPhysAggregateInstance>(r, Kind::PhysicsAggregate);
    Some(r)
}

/// Remove the aggregate's joints and bodies from its world, and the aggregate itself.
pub fn destroy(r: Ref) {
    let Some(a) = get(r) else { return };
    std::mem::take(&mut *a.joints.lock().unwrap()).into_iter().for_each(joint::delete);
    let mut owners = OWNERS.lock().unwrap();
    for (_, b) in &a.bodies {
        owners.remove(&b.addr());
    }
    drop(owners);
    a.bodies.iter().for_each(|(_, b)| body::delete(*b));
    a.handle.free();
    objects::destroy(r);
}

/// Live bodies of an aggregate.
pub fn bodies(r: Ref) -> Vec<body::Ref> {
    get(r).map(|a| a.bodies.iter().map(|(_, b)| *b).filter(|b| body::get(*b).is_some()).collect()).unwrap_or_default()
}
