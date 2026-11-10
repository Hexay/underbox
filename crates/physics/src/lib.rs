//! 3D rigid-body simulation behind s&box's `IPhysicsWorld`, on Rapier. No ABI types here:
//! engine2's physics module binds slots onto [`World`]. Units are s&box's (inches).

mod shape;
mod trace;
mod world;

pub use rapier3d;
pub use rapier3d::math::{Pose, Rotation, Vector};
pub use rapier3d::prelude::{Collider, ColliderHandle as ShapeId, RigidBody, RigidBodyHandle as BodyId};
pub use shape::{Shape, Surface};
pub use trace::{Hit, Sweep};
pub use world::{BodyType, INCHES_PER_METRE, World};
