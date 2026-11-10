//! 3D physics: `IPhysicsWorld`/`IPhysicsBody`/`IPhysicsShape` over the `physics` crate (Rapier).
//! Locking: a world's simulation mutex is never held across a call into managed.

pub mod body;
mod body_slots;
pub mod shape;
pub mod world;

pub fn register(r: &mut abi::Registry) {
    world::register(r);
    body_slots::register(r);
    shape::register(r);
}
