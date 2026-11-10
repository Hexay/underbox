//! `IPhysicsBody`'s mass, centre-of-mass and inertia slots, over [`physics::BodyMass`] overrides.

use super::body::{apply_state, with_rb, with_sim, with_state};
use crate::math::{from_rot, from_vec, to_rot, to_vec};
use abi::Registry;
use abi::bind;
use abi::slots::i_physics_body as b;

pub fn register(r: &mut Registry) {
    bind!(r, b::SET_MASS, |r, m| with_sim(r, |sim, id| sim.set_mass(id, m)));
    bind!(r, b::GET_MASS, |r| with_rb(r, |rb| rb.mass()));
    bind!(r, b::GET_MASS_CENTER, |r| with_rb(r, |rb| from_vec(rb.center_of_mass())));
    bind!(r, b::GET_LOCAL_MASS_CENTER, |r| with_rb(r, |rb| from_vec(rb.local_center_of_mass())));
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
    bind!(r, b::GET_DENSITY, |r| with_sim(r, |sim, id| {
        let first = sim.body(id).and_then(|rb| rb.colliders().first().copied());
        first.and_then(|c| sim.shape_meta(c)).map_or(0.0, |m| m.density)
    }));

    bind!(r, b::GET_LOCAL_INERTIA_VECTOR, |r| with_rb(r, |rb| from_vec(rb.mass_properties().local_mprops.principal_inertia())));
    bind!(r, b::GET_LOCAL_INERTIA_ORIENTATION, |r| {
        let frame = with_rb(r, |rb| Some(rb.mass_properties().local_mprops.principal_inertia_local_frame));
        frame.map_or_else(|| crate::math::identity().rotation, from_rot)
    });
    bind!(r, b::SET_LOCAL_INERTIA, |r, inertia, rotation| {
        with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia = Some((to_vec(inertia), to_rot(rotation)))))
    });
    bind!(r, b::RESET_LOCAL_INERTIA, |r| with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia = None)));
    bind!(r, b::SCALE_INERTIA, |r, scale| with_sim(r, |sim, id| sim.set_mass_overrides(id, |m| m.inertia_scale = scale)));
}
