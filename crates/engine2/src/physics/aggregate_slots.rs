//! `IPhysAggregateInstance`'s slots (PhysicsGroup.cs) and the world/body slots that make, free
//! and find aggregates.

use super::aggregate::{self, Ref, bodies, get};
use super::{body, joint, world};
use crate::math::{from_vec, to_vec};
use crate::model;
use abi::slots::{i_phys_aggregate_instance as a, i_physics_body as pb, i_physics_world as w};
use abi::{HandleId, Registry, StrRet, StringToken, bind, class};
use std::ffi::CStr;

fn each(r: Ref, f: impl Fn(&mut physics::RigidBody)) {
    for b in bodies(r) {
        body::with_rb(b, &f);
    }
}

fn body_handle(b: Option<body::Ref>) -> HandleId<class::IPhysicsBody> {
    b.and_then(body::get).map_or(HandleId::new(-1), |b| b.handle.id())
}

fn nth_body(r: Ref, i: i32) -> Option<(String, body::Ref)> {
    get(r).and_then(|g| g.bodies.get(usize::try_from(i).ok()?).cloned())
}

fn find(r: Ref, pred: impl Fn(&str) -> bool) -> Option<body::Ref> {
    get(r).and_then(|g| g.bodies.iter().find(|(n, _)| pred(n)).map(|(_, b)| *b))
}

/// `SetSurfaceProperties`: every shape of every body uses the surface `name`.
pub(super) fn set_surface(r: Ref, name: StringToken) {
    bodies(r).into_iter().for_each(|b| super::body_slots::set_surface(b, name));
}

/// A body's mass as native reports it: 0 unless dynamic.
fn mass(b: body::Ref) -> f32 {
    body::with_sim(b, |sim, id| sim.reported_mass(id))
}

fn total_mass(r: Ref) -> f32 {
    bodies(r).into_iter().map(mass).sum()
}

/// Mass-weighted centre of the bodies, in world space.
fn mass_center(r: Ref) -> physics::Vector {
    let (m, c) = bodies(r).into_iter().fold((0.0, physics::Vector::ZERO), |(m, c), b| {
        let mb = mass(b);
        (m + mb, c + body::read_rb(b, |rb| rb.center_of_mass()) * mb)
    });
    if m > 0.0 { c / m } else { physics::Vector::ZERO }
}

/// Scale every dynamic body's mass so they sum to `total`.
fn set_total_mass(r: Ref, total: f32) {
    let current = total_mass(r);
    if current <= 0.0 || total <= 0.0 {
        return;
    }
    let k = total / current;
    for b in bodies(r) {
        let m = mass(b);
        if m > 0.0 {
            body::with_sim(b, |sim, id| sim.set_mass_overrides(id, |o| o.mass = Some(m * k)));
        }
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, w::CREATE_AGGREGATE_INSTANCE_1, |world, m, start, _, motion| {
        model::get(m).and_then(|m| m.content.physics.clone()).map_or(HandleId::new(-1), |d| aggregate::create(world, d, start, motion))
    });
    bind!(r, w::CREATE_AGGREGATE_INSTANCE, |world, name: Option<&CStr>, start, _, motion| {
        name.and_then(|n| model::physics::compiled::load(&n.to_string_lossy())).map_or(HandleId::new(-1), |d| aggregate::create(world, d, start, motion))
    });
    bind!(r, w::DESTROY_AGGREGATE_INSTANCE, |_, g| aggregate::destroy(g));
    bind!(r, pb::GET_AGGREGATE, |b| aggregate::of_body(b).and_then(get).map_or(HandleId::new(-1), |g| g.handle.id()));

    bind!(r, a::WAKE_UP, |g| each(g, |rb| rb.wake_up(true)));
    bind!(r, a::PUT_TO_SLEEP, |g| each(g, |rb| rb.sleep()));
    bind!(r, a::IS_ASLEEP, |g| bodies(g).into_iter().all(|b| body::read_rb(b, |rb| rb.is_sleeping())));
    bind!(r, a::SET_VELOCITY, |g, v| each(g, |rb| rb.set_linvel(to_vec(v), true)));
    bind!(r, a::ADD_VELOCITY, |g, v| each(g, |rb| rb.set_linvel(rb.linvel() + to_vec(v), true)));
    bind!(r, a::SET_ANGULAR_VELOCITY, |g, v| each(g, |rb| rb.set_angvel(to_vec(v), true)));
    bind!(r, a::ADD_ANGULAR_VELOCITY, |g, v| each(g, |rb| rb.set_angvel(rb.angvel() + to_vec(v), true)));
    bind!(r, a::SET_LINEAR_DAMPING, |g, d| each(g, |rb| rb.set_linear_damping(d)));
    bind!(r, a::SET_ANGULAR_DAMPING, |g, d| each(g, |rb| rb.set_angular_damping(d)));
    bind!(r, a::GET_TOTAL_MASS, total_mass);
    bind!(r, a::SET_TOTAL_MASS, set_total_mass);
    bind!(r, a::GET_MASS_CENTER, |g| from_vec(mass_center(g)));
    // The root body's position, as native's aggregate origin.
    bind!(r, a::GET_ORIGIN, |g| from_vec(bodies(g).first().map(|b| body::read_rb(*b, |rb| rb.translation())).unwrap_or_default()));
    bind!(r, a::SET_SURFACE_PROPERTIES, set_surface);
    bind!(r, a::GET_WORLD, |g| get(g).and_then(|g| world::get(g.world)).map_or(HandleId::new(-1), |w| w.handle.id()));

    bind!(r, a::GET_BODY_COUNT, |g| get(g).map_or(0, |g| g.bodies.len() as i32));
    bind!(r, a::GET_BODY_HANDLE, |g, i| body_handle(nth_body(g, i).map(|b| b.1)));
    bind!(r, a::GET_BODY_NAME, |g, i| StrRet::from(nth_body(g, i).map(|b| b.0)));
    bind!(r, a::GET_BODY_NAME_HASH, |g, i| nth_body(g, i).map_or(0, |b| StringToken::of(&b.0).0));
    bind!(r, a::GET_BODY_BY_NAME_HASH, |g, hash: i32| body_handle(find(g, |n| StringToken::of(n).0 == hash as u32)));
    bind!(r, a::GET_BODY_INDEX, |g, b| get(g).and_then(|g| g.bodies.iter().position(|x| x.1 == b)).map_or(-1, |i| i as i32));
    bind!(r, a::FIND_BODY_BY_NAME, |g, name: Option<&CStr>| {
        let name = name.map(CStr::to_string_lossy).unwrap_or_default();
        body_handle(find(g, |n| n.eq_ignore_ascii_case(&name)))
    });
    bind!(r, a::GET_JOINT_COUNT, |g| get(g).map_or(0, |g| g.joints.lock().unwrap().len() as i32));
    bind!(r, a::GET_JOINT_HANDLE, |g, i| {
        let j = get(g).and_then(|g| g.joints.lock().unwrap().get(usize::try_from(i).ok()?).copied());
        j.and_then(joint::get).map_or(HandleId::new(-1), |j| j.handle.id())
    });
    // Detaches only: the joint lives on (PhysicsGroup.RemoveJoint).
    bind!(r, a::REMOVE_JOINT, |g, j| get(g).into_iter().for_each(|g| g.joints.lock().unwrap().retain(|x| *x != j)));
}
