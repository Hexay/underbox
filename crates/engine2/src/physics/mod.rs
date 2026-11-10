//! 3D physics: `IPhysicsWorld`/`IPhysicsBody`/`IPhysicsShape`/`IPhysicsJoint` over the `physics`
//! crate (Rapier). Locking: a world's simulation mutex is never held across a call into managed.

pub mod body;
mod body_mass;
mod body_query;
mod body_slots;
mod events;
pub mod joint;
mod joint_drive_slots;
mod joint_slots;
mod query;
mod rules;
pub mod shape;
mod shape_slots;
mod trace;
pub mod world;

pub fn register(r: &mut abi::Registry) {
    world::register(r);
    query::register(r);
    trace::register(r);
    body_slots::register(r);
    body_mass::register(r);
    body_query::register(r);
    shape_slots::register(r);
    joint_slots::register(r);
    joint_drive_slots::register(r);
}
