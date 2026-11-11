//! 3D rigid-body simulation behind s&box's `IPhysicsWorld`, on Rapier. No ABI types here:
//! engine2's physics module binds slots onto [`World`]. Units are s&box's (inches).

mod body_query;
mod contacts;
mod filter;
mod heightfield;
mod joint;
mod mass;
mod rules;
mod scrape;
mod shape;
mod trace;
mod trace_against;
mod world;

#[cfg(test)]
mod contacts_tests;

pub use contacts::{ContactEvent, EventKind};
pub use filter::{DEFAULT_DENSITY, ShapeMeta, function};
pub use heightfield::Grid;
pub use joint::{Joint, JointId, JointKind, Motor, MotorJoint, Parallel, Spring, Wheel};
pub use mass::BodyMass;
pub use rapier3d;
pub use rapier3d::math::{Pose, Rotation, Vector};
pub use rapier3d::prelude::{Collider, ColliderHandle as ShapeId, RigidBody, RigidBodyHandle as BodyId};
pub use rules::{CollisionRules, Rule};
pub use scrape::Scrape;
pub use shape::{Shape, Surface};
pub use trace::{Hit, Sweep};
pub use world::{Activity, BodyType, DEFAULT_GRAVITY, INCHES_PER_METRE, World};
