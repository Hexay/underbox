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
    bind!(r, b::GET_SCRAPE_CONTACTS, |r, buffer: abi::RawPtr<std::ffi::c_void>, max: i32| {
        let max = usize::try_from(max).unwrap_or(0);
        let found = scrape_contacts(r, max);
        // SAFETY: managed passes a pinned span of `max` ScrapeContacts.
        let out = unsafe { buffer.cast::<ScrapeContact>().as_mut_slice(max) };
        let n = found.len().min(out.len());
        out[..n].copy_from_slice(&found[..n]);
        n as i32
    });
}

/// managed's `PhysicsBody3d.ScrapeContact` (sequential, 40 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrapeContact {
    pub other_body: i32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub friction_force: f32,
    pub self_surface: i32,
    pub other_surface: i32,
}
const _: () = assert!(std::mem::size_of::<ScrapeContact>() == 40);

/// Up to `max` of the body's sliding contacts, with each side's surface at the contact.
pub fn scrape_contacts(r: super::body::Ref, max: usize) -> Vec<ScrapeContact> {
    let found = with_sim(r, |sim, id| {
        let user = |c| sim.shape(c).map_or(0, |c| c.user_data);
        let scrapes = sim.scrape_contacts(id, max);
        scrapes.into_iter().map(|s| (s, user(s.shape), user(s.other), sim.triangle_at(s.shape, s.point), sim.triangle_at(s.other, s.point))).collect::<Vec<_>>()
    });
    // A shape with no surface of its own uses `default` (0).
    let surface = |ud, triangle| shape::get(shape::from_user_data(ud)).map_or(-1, |s| s.state.lock().unwrap().surface_at(triangle).max(0));
    let other_body = |ud| shape::get(shape::from_user_data(ud)).and_then(|s| get(s.body)).map_or(-1, |b| b.handle.id::<abi::class::IPhysicsBody>().id);
    found
        .into_iter()
        .map(|(s, mine, theirs, my_tri, their_tri)| ScrapeContact {
            other_body: other_body(theirs),
            point: s.point.to_array(),
            normal: s.normal.to_array(),
            friction_force: s.friction_force,
            self_surface: surface(mine, my_tri),
            other_surface: surface(theirs, their_tri),
        })
        .collect()
}

fn touching(a: shape::Ref, b: shape::Ref, triggers: bool) -> bool {
    let Some(b) = shape::get(b) else { return false };
    shape::with_sim(a, |sim, id| sim.is_touching(id, b.id, triggers))
}
