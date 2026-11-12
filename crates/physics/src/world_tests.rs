use crate::shape::{Shape, Surface};
use crate::world::{Activity, BodyType, World};
use rapier3d::math::{Pose, Vector};

#[test]
fn box_falls_and_rests_on_ground() {
    let mut world = World::default();
    world.set_gravity(Vector::new(0.0, 0.0, -800.0));
    let ground = world.add_body(BodyType::Static, Pose::IDENTITY);
    let flat = Shape::Box { center: Vector::ZERO, rotation: Default::default(), half_extents: Vector::new(500.0, 500.0, 10.0) };
    world.add_shape(ground, &flat, Surface::default()).unwrap();
    let crate_ = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(0.0, 0.0, 200.0)));
    let cube = Shape::Box { center: Vector::ZERO, rotation: Default::default(), half_extents: Vector::splat(16.0) };
    world.add_shape(crate_, &cube, Surface::default()).unwrap();
    for _ in 0..240 {
        world.step(1.0 / 60.0, 1);
    }
    let z = world.body(crate_).unwrap().translation().z;
    assert!((z - 26.0).abs() < 1.0, "resting height {z}, want ~26 (ground top 10 + half 16)");

    let probe = |z| Shape::Sphere { center: Vector::new(0.0, 0.0, z), radius: 4.0 };
    assert_eq!(world.overlapping(&probe(26.0)).len(), 1, "sphere inside the resting cube");
    assert!(world.overlapping(&probe(1000.0)).is_empty());
}

#[test]
fn body_types_round_trip() {
    let mut world = World::default();
    let id = world.add_body(BodyType::Keyframed, Pose::IDENTITY);
    assert_eq!(BodyType::of(world.body(id).unwrap()), BodyType::Keyframed);
    world.set_body_type(id, BodyType::Dynamic);
    assert_eq!(BodyType::of(world.body(id).unwrap()), BodyType::Dynamic);
    assert_eq!(BodyType::from_raw(2), Some(BodyType::Dynamic));
}

#[test]
fn updating_a_shape_moves_its_offset() {
    let mut world = World::default();
    let body = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(100.0, 0.0, 0.0)));
    let id = world.add_shape(body, &Shape::Sphere { center: Vector::ZERO, radius: 8.0 }, Surface::default()).unwrap();
    world.update_shape(id, &Shape::Sphere { center: Vector::new(4.0, 0.0, 10.0), radius: 8.0 });
    assert_eq!(world.shape(id).unwrap().position_wrt_parent().unwrap().translation, Vector::new(4.0, 0.0, 10.0));
    world.step(1.0 / 60.0, 1);
    let (lo, _) = world.shape_bounds(id).unwrap();
    assert!((lo - Vector::new(96.0, -8.0, 2.0)).length() < 1e-3, "{lo:?}");
}

#[test]
fn speeds_are_clamped() {
    let mut world = World::default();
    world.set_max_speed(100.0);
    let ball = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
    world.add_shape(ball, &Shape::Sphere { center: Vector::ZERO, radius: 4.0 }, Surface::default()).unwrap();
    world.body_mut(ball).unwrap().set_linvel(Vector::new(5000.0, 0.0, 0.0), true);
    world.step(1.0 / 60.0, 2);
    assert!((world.body(ball).unwrap().linvel().length() - 100.0).abs() < 1e-3);
}

#[test]
fn activity_reports_moving_bodies_then_sleep() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let ball = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
    world.add_shape(ball, &Shape::Sphere { center: Vector::ZERO, radius: 4.0 }, Surface::default()).unwrap();
    world.step(1.0 / 60.0, 1);
    assert_eq!(world.take_activity(), [Activity { body: ball, user_data: 0, fell_asleep: false }]);
    let fell = (0..600).any(|_| {
        world.step(1.0 / 60.0, 1);
        world.take_activity().iter().any(|a| a.fell_asleep)
    });
    assert!(fell, "a resting body reports falling asleep once");
    world.step(1.0 / 60.0, 1);
    assert!(world.take_activity().is_empty(), "sleeping bodies are quiet");
}
