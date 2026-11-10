//! A ragdoll from a runtime model: its aggregate's bodies and joint exist in the world, hang
//! together under gravity, and go away with it.

use super::aggregate;
use super::world::{self, PhysicsWorld};
use super::{body, joint};
use crate::math::{identity, vec3};
use crate::model::content::Content;
use abi::structs::Transform;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

static NEXT: AtomicI32 = AtomicI32::new(1);

extern "C" fn register_handle(_: *mut c_void, _: u32) -> i32 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

extern "C" fn free_handle(_: i32) {}

/// Stand in for managed's handle index (callbacks 16 and 17).
fn fake_managed() {
    let mut table = vec![0usize; abi::CALLBACK_COUNT];
    table[16] = register_handle as *const () as usize;
    table[17] = free_handle as *const () as usize;
    // SAFETY: the table holds CALLBACK_COUNT entries; only the two set are called here.
    unsafe { abi::rt::callbacks::set_table(table.as_ptr()) };
}

fn new_world() -> world::Ref {
    objects::create_shared(Arc::new(PhysicsWorld::default()))
}

fn ragdoll_model() -> Arc<crate::model::physics::data::PhysicsData> {
    let content = Content { physics: Some(crate::model::tests::ragdoll()), ..Default::default() };
    let model = crate::model::create_runtime("models/test/ragdoll.vmdl", content);
    crate::model::get(model).and_then(|m| m.content.physics.clone()).unwrap()
}

fn position(b: body::Ref) -> physics::Vector {
    body::with_rb(b, |rb| rb.translation())
}

#[test]
fn ragdoll_bodies_and_joint_live_in_the_world() {
    fake_managed();
    let w = new_world();
    let start = Transform { position: vec3(100.0, 0.0, 0.0), ..identity() };
    let r = aggregate::spawn(w, ragdoll_model(), start, 1).unwrap();
    let a = aggregate::get(r).unwrap();
    assert!(a.handle.id::<()>().id >= 0, "registered with managed");

    let names: Vec<_> = a.bodies.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["pelvis", "spine"]);
    let (pelvis, spine) = (a.bodies[0].1, a.bodies[1].1);
    assert_eq!(world::get(w).unwrap().bodies.lock().unwrap().len(), 2);
    assert_eq!(position(spine), physics::Vector::new(100.0, 0.0, 70.0), "start × bind pose");
    assert!(body::with_rb(pelvis, |rb| rb.mass()) > 9.9, "part mass applied");
    assert_eq!(body::get(pelvis).unwrap().shapes.lock().unwrap().len(), 1);

    let joints = a.joints.lock().unwrap().clone();
    assert_eq!(joints.len(), 1);
    let j = joint::get(joints[0]).unwrap();
    assert_eq!((j.body1, j.body2), (pelvis, spine));
    assert!(joint::with(joints[0], |s| Some(s.swing_limit_enabled)).unwrap());
    assert_eq!(aggregate::of_body(spine), Some(r));

    // Falling, the ball joint keeps the bodies 20 apart (anchor 10 above one, 10 below the other).
    for _ in 0..60 {
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    }
    let (p, s) = (position(pelvis), position(spine));
    assert!(p.z < 50.0, "fell: {p:?}");
    assert!((p.distance(s) - 20.0).abs() < 0.5, "held together: {}", p.distance(s));

    aggregate::destroy(r);
    assert!(aggregate::get(r).is_none() && body::get(pelvis).is_none() && joint::get(joints[0]).is_none());
    assert!(world::get(w).unwrap().bodies.lock().unwrap().is_empty());
    assert_eq!(aggregate::of_body(spine), None);
}

#[test]
fn static_motion_and_missing_world() {
    fake_managed();
    let w = new_world();
    let r = aggregate::spawn(w, ragdoll_model(), identity(), 2).unwrap();
    let pelvis = aggregate::get(r).unwrap().bodies[0].1;
    for _ in 0..10 {
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    }
    assert_eq!(position(pelvis).z, 50.0, "static parts stay put");
    aggregate::destroy(r);
    assert!(aggregate::spawn(objects::create_shared::<abi::class::IPhysicsWorld, _>(Arc::new(0u8)), ragdoll_model(), identity(), 1).is_none());
}
