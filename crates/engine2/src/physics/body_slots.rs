//! `IPhysicsBody`'s slots: type, pose, velocity, forces, sleep and switches. Mass is in
//! `body_mass.rs`, shapes in `shape_slots.rs`, queries in `body_query.rs`.

use super::body::{apply_state, get, read_rb, set_motion_locks, with_rb, with_sim, with_state};
use super::shape;
use crate::math::{from_rot, from_vec, to_rot, to_vec, vec3};
use abi::slots::i_physics_body as b;
use abi::structs::Transform;
use abi::{HandleId, Registry, bind};
use physics::{BodyType, Pose, function};

/// Set or clear the touch-event switch on every shape of the body.
fn set_touch(r: super::body::Ref, on: bool) {
    let shapes = get(r).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
    for s in shapes {
        shape::update_meta(s, |m| if on { m.functions |= function::TOUCH } else { m.functions &= !function::TOUCH });
    }
}

/// Every shape of the body uses the surface named `name` (unknown: `default`).
pub fn set_surface(r: super::body::Ref, name: abi::StringToken) {
    let surface = super::surfaces::by_hash(name.0).unwrap_or(-1);
    let shapes = get(r).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
    shapes.into_iter().for_each(|s| shape::set_surface(s, surface));
}

pub fn register(r: &mut Registry) {
    bind!(r, b::GET_WORLD, |r| get(r).and_then(|b| super::world::get(b.world)).map_or(HandleId::new(-1), |w| w.handle.id()));
    bind!(r, b::MANAGED_OBJECT, |r| get(r).map_or(HandleId::new(-1), |b| b.handle.id()));
    bind!(r, b::GET_TYPE, |r| read_rb(r, |rb| BodyType::of(rb) as i64));
    bind!(r, b::SET_TYPE, |r, t| {
        if let Some(kind) = BodyType::from_raw(t) {
            with_sim(r, |sim, id| sim.set_body_type(id, kind));
        }
    });

    bind!(r, b::SET_GRAVITY_SCALE, |r, v| {
        with_state(r, |s| s.gravity_scale = v);
        apply_state(r);
    });
    bind!(r, b::GET_GRAVITY_SCALE, |r| with_state(r, |s| s.gravity_scale));
    bind!(r, b::ENABLE_GRAVITY, |r, v| {
        with_state(r, |s| s.gravity_enabled = v);
        apply_state(r);
    });
    bind!(r, b::IS_GRAVITY_ENABLED, |r| with_state(r, |s| s.gravity_enabled));

    bind!(r, b::SET_POSITION, |r, p| with_rb(r, |rb| rb.set_translation(to_vec(p), true)));
    bind!(r, b::GET_POSITION, |r| read_rb(r, |rb| from_vec(rb.translation())));
    bind!(r, b::SET_ORIENTATION, |r, q| with_rb(r, |rb| rb.set_rotation(to_rot(q), true)));
    bind!(r, b::GET_ORIENTATION, |r| read_rb(r, |rb| Some(from_rot(*rb.rotation()))).unwrap_or_else(|| crate::math::identity().rotation));
    bind!(r, b::SET_TRANSFORM, |r, p, q| with_rb(r, |rb| rb.set_position(Pose::from_parts(to_vec(p), to_rot(q)), true)));
    bind!(r, b::GET_TRANSFORM, |r| {
        let pose = read_rb(r, |rb| Some((rb.translation(), *rb.rotation())));
        pose.map_or_else(crate::math::identity, |(p, q)| Transform { position: from_vec(p), scale: vec3(1.0, 1.0, 1.0), rotation: from_rot(q) })
    });
    bind!(r, b::SET_TARGET_TRANSFORM, |r, p, q, _dt| with_rb(r, |rb| rb.set_next_kinematic_position(Pose::from_parts(to_vec(p), to_rot(q)))));

    bind!(r, b::SET_LINEAR_VELOCITY, |r, v| with_rb(r, |rb| rb.set_linvel(to_vec(v), true)));
    bind!(r, b::GET_LINEAR_VELOCITY, |r| read_rb(r, |rb| from_vec(rb.linvel())));
    bind!(r, b::ADD_LINEAR_VELOCITY, |r, v| with_rb(r, |rb| rb.set_linvel(rb.linvel() + to_vec(v), true)));
    bind!(r, b::GET_VELOCITY_AT_POINT, |r, p| read_rb(r, |rb| from_vec(rb.velocity_at_point(to_vec(p)))));
    bind!(r, b::SET_ANGULAR_VELOCITY, |r, v| with_rb(r, |rb| rb.set_angvel(to_vec(v), true)));
    bind!(r, b::GET_ANGULAR_VELOCITY, |r| read_rb(r, |rb| from_vec(rb.angvel())));
    bind!(r, b::SET_LINEAR_DAMPING, |r, v| with_rb(r, |rb| rb.set_linear_damping(v)));
    bind!(r, b::GET_LINEAR_DAMPING, |r| read_rb(r, |rb| rb.linear_damping()));
    bind!(r, b::SET_ANGULAR_DAMPING, |r, v| with_rb(r, |rb| rb.set_angular_damping(v)));
    bind!(r, b::GET_ANGULAR_DAMPING, |r| read_rb(r, |rb| rb.angular_damping()));

    bind!(r, b::APPLY_LINEAR_IMPULSE, |r, i| with_rb(r, |rb| rb.apply_impulse(to_vec(i), true)));
    bind!(r, b::APPLY_LINEAR_IMPULSE_AT_WORLD_SPACE, |r, i, p| with_rb(r, |rb| rb.apply_impulse_at_point(to_vec(i), to_vec(p), true)));
    bind!(r, b::APPLY_ANGULAR_IMPULSE, |r, i| with_rb(r, |rb| rb.apply_torque_impulse(to_vec(i), true)));
    bind!(r, b::APPLY_FORCE, |r, f| with_rb(r, |rb| rb.add_force(to_vec(f), true)));
    bind!(r, b::APPLY_FORCE_AT, |r, f, p| with_rb(r, |rb| rb.add_force_at_point(to_vec(f), to_vec(p), true)));
    bind!(r, b::APPLY_TORQUE, |r, t| with_rb(r, |rb| rb.add_torque(to_vec(t), true)));
    bind!(r, b::CLEAR_FORCES, |r| with_rb(r, |rb| rb.reset_forces(false)));
    bind!(r, b::CLEAR_TORQUE, |r| with_rb(r, |rb| rb.reset_torques(false)));

    bind!(r, b::WAKE, |r| with_rb(r, |rb| rb.wake_up(true)));
    bind!(r, b::SLEEP, |r| with_rb(r, |rb| rb.sleep()));
    // Real reports static bodies asleep from creation.
    bind!(r, b::IS_SLEEPING, |r| read_rb(r, |rb| rb.is_fixed() || rb.is_sleeping()));
    bind!(r, b::ENABLE_AUTO_SLEEPING, |r| {
        with_state(r, |s| s.auto_sleep = true);
        apply_state(r);
    });
    bind!(r, b::DISABLE_AUTO_SLEEPING, |r| {
        with_state(r, |s| s.auto_sleep = false);
        apply_state(r);
    });
    bind!(r, b::SET_SLEEP_THRESHOLD, |r, v| {
        with_state(r, |s| s.sleep_threshold = v);
        apply_state(r);
    });
    bind!(r, b::GET_SLEEP_THRESHOLD, |r| with_state(r, |s| s.sleep_threshold));
    bind!(r, b::ENABLE_TOUCH_EVENTS, |r| set_touch(r, true));
    bind!(r, b::DISABLE_TOUCH_EVENTS, |r| set_touch(r, false));
    bind!(r, b::IS_TOUCH_EVENT_ENABLED, |r| {
        let shapes = get(r).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
        shapes.into_iter().any(|s| shape::with_meta(s, |m| m.functions & function::TOUCH != 0))
    });

    bind!(r, b::ENABLE, |r| with_rb(r, |rb| rb.set_enabled(true)));
    bind!(r, b::DISABLE, |r| with_rb(r, |rb| rb.set_enabled(false)));
    bind!(r, b::IS_ENABLED, |r| read_rb(r, |rb| rb.is_enabled()));
    bind!(r, b::SET_BULLET, |r, v| with_rb(r, |rb| rb.enable_ccd(v)));
    bind!(r, b::SET_MOTION_LOCKS, |r, x, y, z, pitch, yaw, roll| with_rb(r, |rb| set_motion_locks(rb, [x, y, z, pitch, yaw, roll])));
    bind!(r, b::BUILD_BOUNDS, |r| with_sim(r, |sim, id| sim.body(id).is_some().then(|| super::shape_slots::built_bounds(sim.body_bounds(id)))).unwrap_or_default());
    bind!(r, b::SET_MATERIAL_INDEX, set_surface);
    bind!(r, b::SET_CONTACT_RECYCLING, |_, _| {});
    bind!(r, b::RESET_PROXY, |_| {});
}
