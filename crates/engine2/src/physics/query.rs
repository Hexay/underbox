//! Overlap queries: `IPhysicsWorld.Query*` fill a `CQueryResult` with the shapes they touch.

use super::{shape, world};
use crate::math::to_vec;
use abi::slots::{c_query_result as q, i_physics_world as w};
use abi::{HandleId, NativeRef, Registry, bind, class};
use physics::{BodyType, Pose, Shape};
use std::sync::{Arc, Mutex};

type Ref = NativeRef<class::CQueryResult>;

#[derive(Default)]
struct QueryResult(Mutex<Vec<shape::Ref>>);

fn get(r: Ref) -> Option<Arc<QueryResult>> {
    objects::get::<_, QueryResult>(r)
}

/// `PhysicsQueryObjectSet_t`: which body types a query includes.
fn included(mask: u16, kind: BodyType) -> bool {
    let bit = match kind {
        BodyType::Static => 1 << 0,
        BodyType::Keyframed => 1 << 1,
        BodyType::Dynamic => 1 << 2,
    };
    mask & bit != 0
}

fn query(world: world::Ref, result: Ref, probe: Shape, mask: u16) {
    let found = world::with_sim(world, |sim| {
        let ids = sim.overlapping(&probe).into_iter().filter_map(|id| sim.shape(id));
        let wanted = ids.filter(|c| c.parent().and_then(|b| sim.body(b)).is_some_and(|b| included(mask, BodyType::of(b))));
        wanted.map(|c| shape::from_user_data(c.user_data)).collect::<Vec<_>>()
    });
    if let Some(result) = get(result) {
        result.0.lock().unwrap().extend(found);
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, q::CREATE, || objects::create(QueryResult::default()));
    bind!(r, q::DELETE_THIS, |r| {
        objects::destroy(r);
    });
    bind!(r, q::REMOVE_ALL, |r| get(r).into_iter().for_each(|q| q.0.lock().unwrap().clear()));
    bind!(r, q::COUNT, |r| get(r).map_or(0, |q| q.0.lock().unwrap().len() as i32));
    bind!(r, q::ELEMENT, |r, i| {
        let s = get(r).and_then(|q| usize::try_from(i).ok().and_then(|i| q.0.lock().unwrap().get(i).copied()));
        s.and_then(shape::get).map_or(HandleId::new(-1), |s| s.handle.id())
    });

    bind!(r, w::QUERY, |w, result, center, radius, mask| query(w, result, Shape::Sphere { center: to_vec(center), radius }, mask));
    bind!(r, w::QUERY_1, |w, result, bounds, mask| {
        let (lo, hi) = (to_vec(bounds.mins), to_vec(bounds.maxs));
        let half = (hi - lo) * 0.5;
        let probe = Shape::Box { center: lo + half, rotation: Default::default(), half_extents: half.max(physics::Vector::ZERO) };
        query(w, result, probe, mask)
    });
    // The convex hull of the points (managed passes a frustum's eight corners).
    bind!(r, w::QUERY_2, |w, result, points, count, mask| {
        let points: &[abi::structs::Vector3] = if points.is_null() || count <= 0 {
            &[]
        } else {
            // SAFETY: managed passes `count` Vector3s at `points` for the call (PhysicsWorld3d.Trace.cs).
            unsafe { points.cast().as_slice(count as usize) }
        };
        query(w, result, Shape::Hull { pose: Pose::IDENTITY, points: points.iter().copied().map(to_vec).collect() }, mask)
    });
}
