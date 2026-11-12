//! `IPhysicsBody`'s mass, centre-of-mass and inertia slots, over [`physics::BodyMass`] overrides.

use super::body::{apply_state, with_sim, with_state};
use crate::math::{from_rot, from_vec, to_rot, to_vec};
use abi::Registry;
use abi::bind;
use abi::slots::i_physics_body as b;

pub fn register(r: &mut Registry) {
    bind!(r, b::SET_MASS, |r, m| with_sim(r, |sim, id| sim.set_mass(id, m)));
    bind!(r, b::GET_MASS, |r| with_sim(r, |sim, id| sim.reported_mass(id)));
    bind!(r, b::GET_MASS_CENTER, |r| {
        with_sim(r, |sim, id| from_vec(sim.body(id).map_or(physics::Vector::ZERO, |rb| rb.position().transform_point(sim.reported_local_center(id)))))
    });
    bind!(r, b::GET_LOCAL_MASS_CENTER, |r| with_sim(r, |sim, id| from_vec(sim.reported_local_center(id))));
    bind!(r, b::SET_LOCAL_MASS_CENTER, |r, c| {
        with_state(r, |s| s.local_mass_center = to_vec(c));
        apply_state(r);
    });
    bind!(r, b::SET_OVERRIDE_MASS_CENTER, |r, v| {
        with_state(r, |s| s.override_mass_center = v);
        apply_state(r);
    });
    bind!(r, b::GET_OVERRIDE_MASS_CENTER, |r| with_state(r, |s| s.override_mass_center));
    bind!(r, b::BUILD_MASS, |r| with_sim(r, |sim, id| sim.build_mass(id)));
    bind!(r, b::SET_MASS_UPDATES_DEFERRED, |r, v| with_sim(r, |sim, id| sim.set_mass_deferred(id, v)));
    bind!(r, b::GET_DENSITY, |r| with_sim(r, |sim, id| sim.reported_density(id)));

    bind!(r, b::GET_LOCAL_INERTIA_VECTOR, |r| with_sim(r, |sim, id| from_vec(sim.reported_inertia(id).0)));
    bind!(r, b::GET_LOCAL_INERTIA_ORIENTATION, |r| {
        let frame = with_sim(r, |sim, id| Some(sim.reported_inertia(id).1));
        from_rot(frame.unwrap_or(physics::Rotation::IDENTITY))
    });
    bind!(r, b::SET_LOCAL_INERTIA, |r, inertia, rotation| {
        with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia = Some((to_vec(inertia), to_rot(rotation)))))
    });
    bind!(r, b::RESET_LOCAL_INERTIA, |r| with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia = None)));
    bind!(r, b::SCALE_INERTIA, |r, scale| with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia_scale = scale)));
}
