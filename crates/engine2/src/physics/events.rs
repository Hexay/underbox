//! What a step tells managed: moving and sleeping bodies (`PhysicsEngine.OnActive`), broken joints
//! (`OnPhysicsJointBreak`), and touch/trigger events (`IPhysicsWorld.ProcessIntersections`).
//! Everything is gathered with the world locked and delivered after it's released.

use super::{body, body_cache, joint, shape, world};
use crate::math::{from_rot, from_vec, vec3};
use abi::callbacks::sandbox_physics_physics_engine as engine;
use abi::rt::delegates::{IntersectionNotification, IntersectionSide, IntersectionSink};
use abi::structs::Transform;
use abi::{HandleId, RawPtr, class};
use physics::{ContactEvent, ShapeId};

type Active = (HandleId<class::IPhysicsBody>, Transform, abi::structs::Vector3, abi::structs::Vector3, bool);

/// Step the world, then report activity and broken joints.
pub fn step(r: world::Ref, dt: f32, substeps: u32) {
    let (active, broken) = world::with_sim(r, |sim| {
        sim.step(dt, substeps);
        let active: Vec<Active> = sim
            .take_activity()
            .into_iter()
            .filter_map(|a| {
                let rb = sim.body(a.body)?;
                let handle = body_cache::resolve(body::from_user_data(a.user_data), |e| e.body.handle.id())?;
                let transform = Transform { position: from_vec(rb.translation()), scale: vec3(1.0, 1.0, 1.0), rotation: from_rot(*rb.rotation()) };
                Some((handle, transform, from_vec(rb.linvel()), from_vec(rb.angvel()), a.fell_asleep))
            })
            .collect();
        let broken: Vec<_> = sim.take_broken_joints().into_iter().map(|(_, data)| joint::from_user_data(data)).collect();
        (active, broken)
    });
    for (handle, transform, linear, angular, fell_asleep) in active {
        engine::on_active(handle, transform, linear, angular, fell_asleep, false);
    }
    for j in broken {
        if let Some(id) = joint::get(j).map(|j| j.handle.id()) {
            engine::on_physics_joint_break(id);
        }
    }
}

fn side(sim: &physics::World, id: ShapeId) -> Option<IntersectionSide> {
    let s = shape::from_user_data(sim.shape(id)?.user_data);
    let sh = shape::get(s)?;
    let b = body::get(sh.body)?;
    let surface_index = sh.state.lock().unwrap().surface_index;
    Some(IntersectionSide {
        shape: s.as_raw(),
        body: sh.body.as_raw(),
        surface_index,
        shape_managed_index: sh.handle.id::<()>().id,
        body_managed_index: b.handle.id::<()>().id,
    })
}

fn notification(sim: &physics::World, e: &ContactEvent) -> Option<IntersectionNotification> {
    Some(IntersectionNotification {
        reason: e.kind as i32,
        left: side(sim, e.a)?,
        right: side(sim, e.b)?,
        contact_point: from_vec(e.point),
        contact_speed: from_vec(e.speed),
        surface_normal: from_vec(e.normal),
        contact_normal_speed: e.normal_speed,
        impulse: e.impulse,
    })
}

/// Drain the world's queued touch/trigger events into managed's handler.
pub fn process_intersections(r: world::Ref, sink: RawPtr<std::ffi::c_void>) {
    let batch: Vec<_> = world::with_sim(r, |sim| {
        let events = sim.take_contact_events();
        events.iter().filter_map(|e| notification(sim, e)).collect()
    });
    if let Some(sink) = IntersectionSink::from_raw(sink) {
        sink.deliver(&batch);
    }
}
