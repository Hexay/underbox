//! Slot contracts, called the way managed calls them: through the installed thunk table.

use abi::slots::global_box2d as s;
use abi::structs::{b2BodyDef, b2BodyId, b2Circle, b2ContactEvents, b2Polygon, b2QueryFilter, b2RayResult, b2ShapeDef, b2ShapeId, b2Vec2, b2WorldDef, b2WorldId};
use std::ffi::{CStr, c_char};

/// The thunk behind `$slot`, typed as managed's function pointer for it.
macro_rules! slot {
    ($slot:ident: $ty:ty) => {
        thunk::<$ty>(s::$slot.index)
    };
}

fn thunk<F: Copy>(index: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    // SAFETY: F is the thunk's own extern "C" signature, spelled out at each use.
    unsafe { std::mem::transmute_copy(&crate::igen::test_table()[index]) }
}

fn key(id: b2ShapeId) -> (i32, u16, u16) {
    (id.index1, id.world0, id.generation)
}

const DYNAMIC: i32 = 2;

fn vec2(x: f32, y: f32) -> b2Vec2 {
    b2Vec2 { x, y }
}

fn world(gravity_y: f32) -> b2WorldId {
    let default = slot!(B2_DEFAULT_WORLD_DEF: unsafe extern "C" fn() -> b2WorldDef);
    let create = slot!(B2_CREATE_WORLD: unsafe extern "C" fn(*const b2WorldDef) -> b2WorldId);
    let mut def = unsafe { default() };
    def.gravity = vec2(0.0, gravity_y);
    unsafe { create(&def) }
}

fn body(world: b2WorldId, kind: i32, at: b2Vec2) -> b2BodyId {
    let default = slot!(B2_DEFAULT_BODY_DEF: unsafe extern "C" fn() -> b2BodyDef);
    let create = slot!(B2_CREATE_BODY: unsafe extern "C" fn(b2WorldId, *const b2BodyDef) -> b2BodyId);
    let mut def = unsafe { default() };
    def.r#type = kind;
    def.position = at;
    unsafe { create(world, &def) }
}

/// Box2D's default leaves contact events off; managed turns them on per shape (PhysicsBody2d.Shapes.cs).
fn shape_def() -> b2ShapeDef {
    let mut def = unsafe { slot!(B2_DEFAULT_SHAPE_DEF: unsafe extern "C" fn() -> b2ShapeDef)() };
    def.enable_contact_events = 1;
    def
}

fn add_box(body: b2BodyId, half_w: f32, half_h: f32) -> b2ShapeId {
    let make = slot!(B2_MAKE_BOX: unsafe extern "C" fn(f32, f32) -> b2Polygon);
    let create = slot!(B2_CREATE_POLYGON_SHAPE: unsafe extern "C" fn(b2BodyId, *const b2ShapeDef, *const b2Polygon) -> b2ShapeId);
    let polygon = unsafe { make(half_w, half_h) };
    unsafe { create(body, &shape_def(), &polygon) }
}

fn step(world: b2WorldId) {
    unsafe { slot!(B2_WORLD_STEP: unsafe extern "C" fn(b2WorldId, f32, i32))(world, 1.0 / 60.0, 4) }
}

fn destroy(world: b2WorldId) {
    let destroy = slot!(B2_DESTROY_WORLD: unsafe extern "C" fn(b2WorldId));
    let is_valid = slot!(B2_WORLD_IS_VALID: unsafe extern "C" fn(b2WorldId) -> i32);
    unsafe { destroy(world) };
    assert_eq!(unsafe { is_valid(world) }, 0);
}

#[test]
fn body_falls_under_gravity() {
    let w = world(-10.0);
    let b = body(w, DYNAMIC, vec2(0.0, 10.0));
    let circle = slot!(B2_CREATE_CIRCLE_SHAPE: unsafe extern "C" fn(b2BodyId, *const b2ShapeDef, *const b2Circle) -> b2ShapeId);
    unsafe { circle(b, &shape_def(), &b2Circle { center: vec2(0.0, 0.0), radius: 0.5 }) };

    for _ in 0..60 {
        step(w);
    }
    let y = unsafe { slot!(B2_BODY_GET_POSITION: unsafe extern "C" fn(b2BodyId) -> b2Vec2)(b) }.y;
    assert!((4.0..6.0).contains(&y), "1 s of free fall from y=10 at g=10 ends near y=5, got {y}");
    assert_eq!(unsafe { slot!(B2_BODY_GET_TYPE: unsafe extern "C" fn(b2BodyId) -> i64)(b) }, DYNAMIC as i64);

    let set_name = slot!(B2_BODY_SET_NAME: unsafe extern "C" fn(b2BodyId, *const c_char));
    let get_name = slot!(B2_BODY_GET_NAME: unsafe extern "C" fn(b2BodyId) -> *const c_char);
    unsafe { set_name(b, c"crate".as_ptr()) };
    assert_eq!(unsafe { CStr::from_ptr(get_name(b)) }, c"crate");
    destroy(w);
}

#[test]
fn contacts_are_reported() {
    let w = world(-10.0);
    let ground = add_box(body(w, 0, vec2(0.0, 0.0)), 5.0, 0.5);
    let crate_ = add_box(body(w, DYNAMIC, vec2(0.0, 2.0)), 0.5, 0.5);
    let events = slot!(B2_WORLD_GET_CONTACT_EVENTS: unsafe extern "C" fn(b2WorldId) -> b2ContactEvents);

    let mut touched = None;
    for _ in 0..120 {
        step(w);
        let e = unsafe { events(w) };
        if e.begin_count > 0 {
            let begin = e.begin_events as *const box2d_sys::b2ContactBeginTouchEvent;
            touched = Some(unsafe { *begin });
            break;
        }
    }
    let begin = touched.expect("the falling box lands on the ground within 2 s");
    let pair = [key(super::cast(begin.shapeIdA)), key(super::cast(begin.shapeIdB))];
    assert!(pair.contains(&key(ground)) && pair.contains(&key(crate_)), "{pair:?}");
    destroy(w);
}

#[test]
fn ray_casts_hit() {
    let w = world(0.0);
    let target = add_box(body(w, 0, vec2(3.0, 0.0)), 1.0, 1.0);
    let filter = unsafe { slot!(B2_DEFAULT_QUERY_FILTER: unsafe extern "C" fn() -> b2QueryFilter)() };
    let cast = slot!(B2_WORLD_CAST_RAY_CLOSEST: unsafe extern "C" fn(b2WorldId, *const b2Vec2, *const b2Vec2, *const b2QueryFilter) -> b2RayResult);

    let hit = unsafe { cast(w, &vec2(-5.0, 0.0), &vec2(20.0, 0.0), &filter) };
    assert_eq!(hit.hit, 1);
    assert_eq!(key(hit.shape_id), key(target));
    assert!((hit.point.x - 2.0).abs() < 0.01 && (hit.normal.x + 1.0).abs() < 0.01, "{hit:?}");

    let miss = unsafe { cast(w, &vec2(-5.0, 5.0), &vec2(20.0, 0.0), &filter) };
    assert_eq!(miss.hit, 0);
    destroy(w);
}
