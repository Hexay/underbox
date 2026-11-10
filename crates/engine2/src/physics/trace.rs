//! `PhysicsTrace.Trace*`: rays and shape sweeps against a world. Native-side filters (triggers,
//! body type, tags) run inside the world lock; managed's filter delegate runs after, outside it.

use super::{body, shape, world};
use crate::math::{from_vec, to_rot, to_vec};
use abi::rt::delegates::TraceFilter;
use abi::slots::physics_trace as t;
use abi::structs::{PhysicsTrace_Result, Request};
use abi::{NativeRef, Registry, bind};
use physics::{BodyType, Collider, Hit, Shape, ShapeId, Sweep};

/// managed's `PhysicsTrace.Request.ShapeType`.
const SPHERE: i32 = 0;
const BOX: i32 = 1;
const CAPSULE: i32 = 2;

fn sweep(req: &Request) -> Sweep {
    let s = &req.start_shape;
    let (mins, maxs) = (to_vec(s.mins), to_vec(s.maxs));
    let probe = match s.r#type {
        SPHERE if s.radius.vec.x <= 0.0 => None,
        SPHERE => Some(Shape::Sphere { center: physics::Vector::ZERO, radius: s.radius.vec.x }),
        BOX => Some(Shape::Box { center: (mins + maxs) * 0.5, rotation: physics::Rotation::IDENTITY, half_extents: ((maxs - mins) * 0.5).abs() }),
        CAPSULE => Some(Shape::Capsule { a: mins, b: maxs, radius: s.radius.vec.x }),
        // Cylinder; a zero second radius makes it a cone (PhysicsTraceBuilder).
        _ if s.radius.vec.y == 0.0 => Some(Shape::Cone { half_height: maxs.z, radius: s.radius.vec.x }),
        _ => Some(Shape::Cylinder { half_height: maxs.z, radius: s.radius.vec.x }),
    };
    let probe = traced_body(req).and_then(|b| b.shapes.lock().unwrap().first().and_then(|s| shape::get(*s))).map(|s| s.geometry.lock().unwrap().clone()).or(probe);
    Sweep { probe, rotation: to_rot(s.start_rot), start: to_vec(req.start_pos), end: to_vec(req.end_pos) }
}

/// The body a `Body(...)` trace sweeps; it never hits itself.
// TODO: sweep every shape on the body as a compound, not just its first.
fn traced_body(req: &Request) -> Option<std::sync::Arc<body::Body>> {
    body::get(NativeRef::from_raw(req.body.self_ as *mut _))
}

/// Whether native-side filters let the trace hit `c`. Takes no managed call.
fn passes(req: &Request, sim: &physics::World, own: Option<physics::BodyId>, c: &Collider) -> bool {
    let sensor_ok = match req.trigger_filter {
        0 => !c.is_sensor(),
        2 => c.is_sensor(),
        _ => true,
    };
    let parent = c.parent();
    let ignored_type = parent.and_then(|b| sim.body(b)).is_some_and(|b| {
        let bit = match BodyType::of(b) {
            BodyType::Static => 0,
            BodyType::Dynamic => 1,
            BodyType::Keyframed => 2,
        };
        req.object_set_mask & (1 << bit) != 0
    });
    if !sensor_ok || ignored_type || (own.is_some() && parent == own) {
        return false;
    }
    let Some(s) = shape::get(shape::from_user_data(c.user_data)) else { return false };
    let state = s.state.lock().unwrap();
    let has = |tag: &u32| state.tags.contains(tag);
    !state.ignore_traces
        && tags(&req.tag_require).all(has)
        && (tags(&req.tag_any).next().is_none() || tags(&req.tag_any).any(has))
        && !tags(&req.tag_exclude).any(has)
}

/// A request's tag list: zero entries are unused slots.
fn tags(list: &[u32; 8]) -> impl Iterator<Item = &u32> {
    list.iter().filter(|t| **t != 0)
}

/// A hit and the shape object it landed on (from its collider's user_data, see shape::add).
type Found = (Hit, shape::Ref);

/// Every candidate hit (or just the nearest, without a managed filter), with the world locked.
fn candidates(req: &Request, all: bool) -> Vec<Found> {
    let sweep = sweep(req);
    let own = traced_body(req).map(|b| b.id);
    let wants_all = all || TraceFilter::from_raw(req.filter_delegate).is_some();
    world::with_sim(NativeRef::from_raw(req.world.self_ as *mut _), |sim| {
        let sim: &physics::World = sim;
        let keep = |_: ShapeId, c: &Collider| passes(req, sim, own, c);
        let hits = if wants_all { sim.trace_all(&sweep, &keep) } else { sim.trace(&sweep, &keep).into_iter().collect() };
        let owner = |h: &Hit| shape::from_user_data(sim.shape(h.shape).map_or(0, |c| c.user_data));
        hits.into_iter().map(|h| (h, owner(&h))).collect()
    })
}

fn handle_id(s: shape::Ref) -> i32 {
    shape::get(s).map_or(-1, |s| s.handle.id::<()>().id)
}

fn result(req: &Request, found: Option<&Found>) -> PhysicsTrace_Result {
    let mut out = PhysicsTrace_Result::default();
    (out.start_pos, out.end_pos, out.hit_pos, out.fraction) = (req.start_pos, req.end_pos, req.end_pos, 1.0);
    (out.physics_body_handle, out.physics_shape_handle, out.surface_property, out.triangle_index) = (-1, -1, -1, -1);
    let Some((hit, owner)) = found else { return out };
    out.end_pos = from_vec(hit.position);
    out.hit_pos = from_vec(if req.use_hit_position != 0 { hit.point } else { hit.position });
    out.normal = from_vec(hit.normal);
    out.fraction = hit.fraction;
    out.started_in_solid = u8::from(hit.started_in_solid);
    if let Some(s) = shape::get(*owner) {
        out.physics_shape_handle = s.handle.id::<()>().id;
        out.physics_body_handle = body::get(s.body).map_or(-1, |b| b.handle.id::<()>().id);
        for (slot, tag) in out.tags.iter_mut().zip(s.state.lock().unwrap().tags.iter()) {
            *slot = *tag;
        }
    }
    out
}

/// The hits managed's filter (if any) allows, nearest first. Runs with no lock held.
fn allowed(req: &Request, found: Vec<Found>) -> impl Iterator<Item = Found> {
    let filter = TraceFilter::from_raw(req.filter_delegate);
    found.into_iter().filter(move |(_, s)| filter.is_none_or(|f| f.allows(handle_id(*s))))
}

fn trace(req: Request) -> PhysicsTrace_Result {
    let first = allowed(&req, candidates(&req, false)).next();
    result(&req, first.as_ref())
}

pub fn register(r: &mut Registry) {
    bind!(r, t::TRACE, trace);
    bind!(r, t::TRACE_ALL, |req, out| {
        let hits: Vec<_> = allowed(&req, candidates(&req, true)).map(|f| result(&req, Some(&f))).collect();
        crate::containers::with(out, |v: &mut Vec<PhysicsTrace_Result>| v.extend(hits));
    });
}
