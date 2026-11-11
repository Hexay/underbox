use crate::contacts::EventKind;
use crate::filter::function;
use crate::rules::{CollisionRules, Rule};
use crate::shape::{Shape, Surface};
use crate::world::{BodyType, World};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};

const PLAYER: u32 = 10;
const GHOST: u32 = 11;
const WATER: u32 = 12;

fn slab(world: &mut World, trigger: bool, tags: &[u32]) -> ColliderHandle {
    let body = world.add_body(BodyType::Static, Pose::IDENTITY);
    let shape = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(100.0, 100.0, 10.0) };
    let id = world.add_shape(body, &shape, Surface { trigger, ..Default::default() }).unwrap();
    world.update_shape_meta(id, |m| m.tags = tags.to_vec());
    id
}

fn ball(world: &mut World, z: f32, tags: &[u32]) -> (RigidBodyHandle, ColliderHandle) {
    let body = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(0.0, 0.0, z)));
    let id = world.add_shape(body, &Shape::Sphere { center: Vector::ZERO, radius: 5.0 }, Surface::default()).unwrap();
    world.update_shape_meta(id, |m| m.tags = tags.to_vec());
    (body, id)
}

fn run(world: &mut World, steps: usize) -> Vec<EventKind> {
    (0..steps)
        .flat_map(|_| {
            world.step(1.0 / 60.0, 1);
            world.take_contact_events()
        })
        .map(|e| e.kind)
        .collect()
}

#[test]
fn trigger_reports_enter_then_exit() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    let zone = slab(&mut world, true, &[]);
    let (body, shape) = ball(&mut world, 60.0, &[]);
    world.body_mut(body).unwrap().set_linvel(Vector::new(0.0, 0.0, -600.0), true);
    let mut seen = Vec::new();
    for _ in 0..30 {
        world.step(1.0 / 60.0, 1);
        for e in world.take_contact_events() {
            assert_eq!([e.a, e.b].iter().filter(|c| **c == zone || **c == shape).count(), 2);
            seen.push(e.kind);
        }
        if seen.len() == 1 {
            assert!(world.is_touching(zone, shape, true), "inside the trigger");
        }
    }
    assert_eq!(seen, [EventKind::TriggerBegin, EventKind::TriggerEnd]);
    assert!(world.body(body).unwrap().translation().z < -20.0, "triggers don't block");
}

#[test]
fn ignored_pairs_pass_through_and_collide_pairs_rest() {
    let mut world = World::default();
    world.set_collision_rules(CollisionRules::new([], [(PLAYER, GHOST, Rule::Ignore)]));
    slab(&mut world, false, &[GHOST]);
    let (ghost, _) = ball(&mut world, 30.0, &[PLAYER]);
    let (other, _) = ball(&mut world, 30.0, &[]);
    world.body_mut(other).unwrap().set_translation(Vector::new(50.0, 0.0, 30.0), true);
    let events = run(&mut world, 120);
    assert!(world.body(ghost).unwrap().translation().z < -50.0, "player falls through ghost");
    assert!((world.body(other).unwrap().translation().z - 15.0).abs() < 1.0, "untagged ball rests on top");
    assert_eq!(events, [EventKind::TouchBegin], "only the resting ball touches");
}

#[test]
fn trigger_rule_between_solids_reports_but_does_not_block() {
    let mut world = World::default();
    world.set_collision_rules(CollisionRules::new([(WATER, Rule::Trigger)], []));
    slab(&mut world, false, &[WATER]);
    let (body, _) = ball(&mut world, 30.0, &[]);
    let events = run(&mut world, 120);
    assert!(world.body(body).unwrap().translation().z < -50.0);
    assert_eq!(events, [EventKind::TriggerBegin, EventKind::TriggerEnd]);
}

#[test]
fn touch_events_follow_the_function_mask() {
    let mut world = World::default();
    let ground = slab(&mut world, false, &[]);
    let (_, shape) = ball(&mut world, 30.0, &[]);
    for id in [ground, shape] {
        world.update_shape_meta(id, |m| m.functions = function::SOLID);
    }
    assert!(run(&mut world, 120).is_empty(), "touch events off");
    assert!(world.is_touching(ground, shape, false), "still tracked for IsTouching");

    world.update_shape_meta(shape, |m| m.functions = function::SOLID | function::TOUCH | function::PERSISTS);
    let events = run(&mut world, 3);
    assert_eq!(events, [EventKind::TouchPersists; 3]);

    world.update_shape_meta(shape, |m| m.functions = function::TOUCH);
    assert_eq!(run(&mut world, 1), [EventKind::TouchEnd], "dropping SOLID ends the touch");
}

/// sbox-walker's teleport trigger overlaps its floor and a keyframed coin; reporting either pair
/// teleported the coin's root, the whole map.
#[test]
fn static_trigger_ignores_static_and_keyframed_but_sees_dynamic() {
    let mut world = World::default();
    world.set_gravity(Vector::ZERO);
    slab(&mut world, true, &[]);
    slab(&mut world, false, &[]);
    let coin = world.add_body(BodyType::Keyframed, Pose::from_translation(Vector::new(50.0, 0.0, 5.0)));
    world.add_shape(coin, &Shape::Sphere { center: Vector::ZERO, radius: 5.0 }, Surface::default()).unwrap();
    assert_eq!(run(&mut world, 5), [], "no static or keyframed trigger touch");
    let (body, _) = ball(&mut world, 60.0, &[]);
    world.body_mut(body).unwrap().set_linvel(Vector::new(0.0, 0.0, -600.0), true);
    assert!(run(&mut world, 10).contains(&EventKind::TriggerBegin));
}

#[test]
fn touch_event_carries_contact_data() {
    let mut world = World::default();
    let ground = slab(&mut world, false, &[]);
    let (_, shape) = ball(&mut world, 30.0, &[]);
    let begin = (0..120)
        .find_map(|_| {
            world.step(1.0 / 60.0, 1);
            world.take_contact_events().into_iter().next()
        })
        .unwrap();
    assert_eq!(begin.kind, EventKind::TouchBegin);
    let (top, ball) = if begin.a == ground { (Vector::Z, shape) } else { (-Vector::Z, ground) };
    assert!(begin.b == ball);
    assert!((begin.normal - top).length() < 1e-3, "normal from a to b: {:?}", begin.normal);
    assert!((begin.point.z - 10.0).abs() < 1.0, "contact on the ground's top: {:?}", begin.point);
}
