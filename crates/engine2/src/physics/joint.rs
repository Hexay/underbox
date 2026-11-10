//! `IPhysicsJoint`: a constraint between two bodies, over [`physics::Joint`] settings.

use super::{body, world};
use crate::handles::{Handle, Kind};
use crate::math::{to_rot, to_vec};
use abi::structs::Transform;
use abi::{HandleId, NativeRef, class};
use physics::{Joint as Settings, JointId, JointKind, Pose};
use std::sync::Arc;

pub type Ref = NativeRef<class::IPhysicsJoint>;

pub struct Joint {
    pub world: world::Ref,
    pub id: JointId,
    pub body1: body::Ref,
    pub body2: body::Ref,
    pub handle: Handle,
}

pub fn get(r: Ref) -> Option<Arc<Joint>> {
    objects::get::<_, Joint>(r)
}

/// The joint a Rapier joint belongs to: `create` stores its object's address in `user_data`.
pub fn from_user_data(data: u128) -> Ref {
    Ref::from_raw(data as usize as *mut _)
}

fn pose(t: Transform) -> Pose {
    Pose::from_parts(to_vec(t.position), to_rot(t.rotation))
}

/// Read the joint's settings (the world locked).
pub fn with<R: Default>(r: Ref, f: impl FnOnce(&Settings) -> R) -> R {
    let Some(j) = get(r) else { return R::default() };
    world::with_sim(j.world, |sim| sim.joint(j.id).map(f).unwrap_or_default())
}

/// Change the joint's settings; the simulation rebuilds it.
pub fn update(r: Ref, f: impl FnOnce(&mut Settings)) {
    let Some(j) = get(r) else { return };
    world::with_sim(j.world, |sim| sim.update_joint(j.id, f));
}

/// Run `f` on the joint's simulation with its id (the world locked).
pub fn with_sim<R: Default>(r: Ref, f: impl FnOnce(&mut physics::World, JointId) -> R) -> R {
    let Some(j) = get(r) else { return R::default() };
    world::with_sim(j.world, |sim| f(sim, j.id))
}

pub fn create(world_ref: world::Ref, kind: JointKind, a: body::Ref, b: body::Ref, frame_a: Transform, frame_b: Transform) -> HandleId<class::IPhysicsJoint> {
    let (Some(w), Some(ba), Some(bb)) = (world::get(world_ref), body::get(a), body::get(b)) else { return HandleId::new(-1) };
    let settings = Settings::new(kind, pose(frame_a), pose(frame_b));
    let Some(id) = w.sim.lock().unwrap().add_joint(ba.id, bb.id, settings) else { return HandleId::new(-1) };
    let joint = Arc::new(Joint { world: world_ref, id, body1: a, body2: b, handle: Handle::default() });
    let r: Ref = objects::create_shared(joint.clone());
    w.sim.lock().unwrap().set_joint_user_data(id, r.addr() as u128);
    w.joints.lock().unwrap().push(r);
    // Registered with the world unlocked: managed's RegisterHandle may call straight back in.
    joint.handle.register(r, Kind::PhysicsJoint)
}

/// Remove a joint from its world and drop its object and managed handle.
pub fn delete(r: Ref) {
    let Some(j) = get(r) else { return };
    if let Some(w) = world::get(j.world) {
        w.sim.lock().unwrap().remove_joint(j.id);
        w.joints.lock().unwrap().retain(|x| *x != r);
    }
    j.handle.free();
    objects::destroy(r);
}

/// Delete the joints on `body` (removing a body removes its joints).
pub fn forget_attached(world_ref: world::Ref, body: body::Ref) {
    let Some(w) = world::get(world_ref) else { return };
    let joints = w.joints.lock().unwrap().clone();
    for r in joints {
        if get(r).is_some_and(|j| j.body1 == body || j.body2 == body) {
            delete(r);
        }
    }
}
