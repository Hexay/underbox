//! `IPhysicsBody`'s queries against other bodies and shapes, and buoyancy.

use super::body::{get, with_sim};
use super::shape;
use crate::math::{from_vec, to_rot, to_vec};
use abi::slots::i_physics_body as b;
use abi::{Registry, bind};
use physics::Pose;

pub fn register(r: &mut Registry) {
    bind!(r, b::GET_CLOSEST_POINT, |r, p| with_sim(r, |sim, id| sim.closest_point(id, to_vec(p)).map(from_vec)).unwrap_or(p));
    bind!(r, b::CHECK_OVERLAP, |r, other, t| {
        let Some(other) = get(other) else { return false };
        with_sim(r, |sim, id| sim.bodies_overlap(id, other.id, Pose::from_parts(to_vec(t.position), to_rot(t.rotation))))
    });
    bind!(r, b::COMPUTE_PENETRATION, |r, other, t, direction, distance| {
        let Some(other) = get(other) else { return false };
        let found = with_sim(r, |sim, id| sim.penetration(id, other.id, Pose::from_parts(to_vec(t.position), to_rot(t.rotation))));
        let Some((dir, depth)) = found else { return false };
        (*direction, *distance) = (from_vec(dir), depth);
        true
    });
    bind!(r, b::IS_TOUCHING, |r, other, triggers| {
        let Some(other) = get(other) else { return false };
        let theirs = other.shapes.lock().unwrap().clone();
        let ours = get(r).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
        ours.into_iter().any(|s| theirs.iter().any(|o| touching(s, *o, triggers)))
    });
    bind!(r, b::IS_TOUCHING_1, |r, other, triggers| {
        let ours = get(r).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
        ours.into_iter().any(|s| touching(s, other, triggers))
    });
    bind!(r, b::APPLY_BUOYANCY_IMPULSE, |r, origin, normal, density, linear_drag, angular_drag, velocity, gravity, dt| {
        with_sim(r, |sim, id| sim.apply_buoyancy(id, to_vec(origin), to_vec(normal), density, linear_drag, angular_drag, to_vec(velocity), to_vec(gravity), dt))
    });
}

fn touching(a: shape::Ref, b: shape::Ref, triggers: bool) -> bool {
    let Some(b) = shape::get(b) else { return false };
    shape::with_sim(a, |sim, id| sim.is_touching(id, b.id, triggers))
}
