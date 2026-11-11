//! `SceneAggregateObject`: one scene object drawing many fragments of a model's first mesh, each
//! one of its draw calls with its own transform and tint (render/draw/dynamic.rs). Native culls
//! them per fragment and submits one indirect draw; drawing them one by one looks the same.

use super::object::{self, SceneObject, State};
use crate::handles::Kind;
use abi::structs::{BBox, Transform, Vector3, Vector4};
use abi::{HandleId, RawPtr, Registry, bind, class};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, Weak};

/// Managed's `AggregateFragment` (SceneAggregateObject.cs).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Fragment {
    pub transform: Transform,
    pub mins: Vector3,
    pub maxs: Vector3,
    pub tint: Vector4,
    /// The draw call of the model's first mesh this fragment draws.
    pub draw: i32,
    pub flags: i32,
}

const _: () = assert!(size_of::<Fragment>() == 88);

type Entry = (Weak<SceneObject>, Arc<[Fragment]>);

static TABLE: Mutex<Option<HashMap<usize, Entry>>> = Mutex::new(None);

/// Object `r`'s fragments, when it is an aggregate.
pub fn fragments(r: object::Ref) -> Option<Arc<[Fragment]>> {
    let table = TABLE.lock().unwrap();
    let (object, f) = table.as_ref()?.get(&r.addr())?;
    (object.strong_count() > 0).then(|| f.clone())
}

fn bounds(fragments: &[Fragment]) -> BBox {
    let (lo, hi) = fragments.iter().fold((glam::Vec3::INFINITY, glam::Vec3::NEG_INFINITY), |(lo, hi), f| (lo.min(crate::math::v3(f.mins)), hi.max(crate::math::v3(f.maxs))));
    BBox { mins: crate::math::from_v3(lo), maxs: crate::math::from_v3(hi) }
}

/// Null (managed throws "needs a single mesh") unless every fragment names a draw call of the
/// model's first mesh.
fn create(w: super::world::Ref, m: crate::model::Ref, p: RawPtr<c_void>, n: i32, _shadows: bool) -> HandleId<class::CSceneObject> {
    // SAFETY: managed passes a pinned `AggregateFragment[n]` (SceneAggregateObject's constructor).
    let fragments: Arc<[Fragment]> = unsafe { p.cast::<Fragment>().as_slice(n.max(0) as usize) }.into();
    let calls = crate::model::get(m).and_then(|model| model.content.meshes.first().map(|e| e.mesh.draw_calls().len())).unwrap_or(0);
    if fragments.is_empty() || fragments.iter().any(|f| usize::try_from(f.draw).map_or(true, |d| d >= calls)) {
        return HandleId::new(0);
    }
    let state = State { model: crate::model::copy(m), bounds: bounds(&fragments), ..State::default() };
    let (r, id) = object::create_native::<class::CSceneObject, class::CSceneObject>(w, Kind::SceneObject, state, None);
    let obj = object::get(r).expect("just created");
    let mut table = TABLE.lock().unwrap();
    let table = table.get_or_insert_with(HashMap::new);
    table.retain(|_, (o, _)| o.strong_count() > 0);
    table.insert(r.addr(), (Arc::downgrade(&obj), fragments));
    id
}

pub fn register(r: &mut Registry) {
    bind!(r, abi::slots::aggregate_glue::CREATE_AGGREGATE_SCENE_OBJECT, create);
}
