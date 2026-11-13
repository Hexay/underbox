//! A ragdoll from a runtime model: its aggregate's bodies and joint exist in the world, hang
//! together under gravity, and go away with it.

use super::aggregate;
use super::world::{self, PhysicsWorld};
use super::{body, joint};
use crate::math::{identity, vec3};
use crate::model::content::Content;
use crate::model::physics::data::{Indices, JointDesc, Part, PhysicsData, SphereDesc};
use abi::structs::Transform;
use crate::handles::fake_managed;
use std::sync::Arc;

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

fn ball(z: f32, x: f32, surface: u32) -> Part {
    let mut p = Part { bone: format!("b{x}"), bind: Transform { position: vec3(x, 0.0, z), ..identity() }, mass: 10.0, ..Part::default() };
    p.shapes.spheres.push(SphereDesc { center: glam::Vec3::ZERO, radius: 4.0, ids: Indices { surface, attribute: 0 } });
    p
}

#[test]
fn wheel_joints_spawn_spin_and_hold_the_axle() {
    fake_managed();
    let w = new_world();
    let mut j = JointDesc { kind: 16, body1: 0, body2: 1, frame1: Transform { position: vec3(20.0, 0.0, 0.0), ..identity() }, ..JointDesc::default() };
    (j.enable_angular_motor, j.angular_target_velocity, j.max_torque) = (true, vec3(5.0, 0.0, 0.0), 1.0e7);
    let data = PhysicsData::new("car", vec![ball(50.0, 0.0, 0), ball(50.0, 20.0, 0)], vec![j], vec![0]);
    let r = aggregate::spawn(w, data, identity(), 1).unwrap();
    let a = aggregate::get(r).unwrap();
    let joint = a.joints.lock().unwrap()[0];
    assert!(joint::with(joint, |s| Some(s.kind == physics::JointKind::Wheel && s.wheel.spin_motor)).unwrap());
    for _ in 0..60 {
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    }
    let spin = joint::with_sim(joint, |sim, id| sim.joint_relative_spin(id));
    assert!((spin.x - 5.0).abs() < 0.1 && spin.y.abs() < 0.1 && spin.z.abs() < 0.1, "spins about frame X: {spin:?}");
    let (chassis, wheel) = (position(a.bodies[0].1), position(a.bodies[1].1));
    assert!(chassis.z < 50.0, "fell: {chassis:?}");
    assert!(((wheel - chassis).length() - 20.0).abs() < 0.2, "axle held: {:?}", wheel - chassis);
    aggregate::destroy(r);
}

#[test]
fn shapes_take_their_descriptor_surface() {
    fake_managed();
    let w = new_world();
    let data = PhysicsData::new("pebble", vec![ball(0.0, 0.0, 1)], vec![], vec![0, 3]);
    let r = aggregate::spawn(w, data, identity(), 2).unwrap();
    let b = aggregate::get(r).unwrap().bodies[0].1;
    let s = body::get(b).unwrap().shapes.lock().unwrap()[0];
    assert_eq!(super::shape::with_state(s, |st| st.surface_index), 3, "ids.surface indexes the data's surface list");
    aggregate::destroy(r);
}

#[test]
fn sliding_bodies_report_scrape_contacts_with_their_surfaces() {
    fake_managed();
    let w = new_world();
    let ground = aggregate::spawn(w, PhysicsData::new("floor", vec![Part { mass: 0.0, ..ball(-100.0, 0.0, 1) }], vec![], vec![0, 0]), identity(), 2).unwrap();
    let floor = aggregate::get(ground).unwrap().bodies[0].1;
    let crate_ = aggregate::spawn(w, PhysicsData::new("crate", vec![ball(-91.0, 0.0, 0)], vec![], vec![0]), identity(), 1).unwrap();
    let b = aggregate::get(crate_).unwrap().bodies[0].1;
    for _ in 0..20 {
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    }
    body::with_rb(b, |rb| rb.set_linvel(physics::Vector::new(150.0, 0.0, 0.0), true));
    world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    let s = super::body_query::scrape_contacts(b, 8);
    assert!(!s.is_empty(), "the ball slides on the floor ball");
    assert_eq!(s[0].other_body, body::get(floor).unwrap().handle.id::<abi::class::IPhysicsBody>().id);
    assert!(s[0].friction_force > 0.0 && (s[0].self_surface, s[0].other_surface) == (0, 0), "{:?}", s[0]);
    assert!(super::body_query::scrape_contacts(b, 0).is_empty());
}

#[test]
fn set_surface_properties_reaches_every_shape() {
    fake_managed();
    let w = new_world();
    let r = aggregate::spawn(w, ragdoll_model(), identity(), 1).unwrap();
    super::surfaces::add("aggregate_test_rubber", "default", "");
    super::aggregate_slots::set_surface(r, abi::StringToken::of("aggregate_test_rubber"));
    let want = super::surfaces::by_hash(abi::StringToken::of("aggregate_test_rubber").0).unwrap();
    for (_, b) in aggregate::get(r).unwrap().bodies.iter() {
        for s in body::get(*b).unwrap().shapes.lock().unwrap().iter() {
            assert_eq!(super::shape::with_state(*s, |st| st.surface_at(None)), want);
        }
    }
    aggregate::destroy(r);
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
