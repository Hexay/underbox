//! `ISceneWorld.MeshTrace`/`MeshTraceAll` (managed's `MeshTraceRequest` on a world): the segment
//! against every rendering model object's triangles (the model trace, in each object's space).
//! Objects are filtered by the input's tag sets, then by its filter delegate (`byte (int handle)`).

use crate::scene::{object, world as scene_world};
use abi::structs::{MeshTraceInput, MeshTraceOutput, Transform};
use abi::{NativeRef, Registry, bind, class};
use glam::Affine3A;

fn affine(t: &Transform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(crate::math::v3(t.scale), crate::math::quat(t.rotation), crate::math::v3(t.position))
}

/// `require` all, `any` (if set) one, `exclude` none; zero entries are unused.
fn tags_pass(tags: &[u32], input: &MeshTraceInput) -> bool {
    let set = |list: &[u32; 8]| list.iter().copied().filter(|&t| t != 0).collect::<Vec<_>>();
    let (require, any, exclude) = (set(&input.tag_require), set(&input.tag_any), set(&input.tag_exclude));
    require.iter().all(|t| tags.contains(t)) && (any.is_empty() || any.iter().any(|t| tags.contains(t))) && !exclude.iter().any(|t| tags.contains(t))
}

fn accepted(filter: usize, handle: i32) -> bool {
    if filter == 0 {
        return true;
    }
    // SAFETY: managed's `MeshTraceRequest` passes an UnmanagedCallersOnly `byte (int)` it keeps
    // alive for the call.
    let f: unsafe extern "C" fn(i32) -> u8 = unsafe { std::mem::transmute(filter) };
    unsafe { f(handle) != 0 }
}

/// Every object hit, nearest first.
pub fn hits(w: scene_world::Ref, input: &MeshTraceInput) -> Vec<MeshTraceOutput> {
    let Some(world) = scene_world::get(w) else { return Vec::new() };
    let addrs = world.objects.lock().unwrap().clone();
    // Gather first: the filter calls into managed, which may call back into these objects.
    let candidates: Vec<(i32, NativeRef<class::CModel>, Transform)> = addrs
        .into_iter()
        .filter_map(|a| object::get(NativeRef::from_raw(a as *mut _)))
        .filter_map(|so| {
            let s = so.state.lock().unwrap();
            (s.rendering && !s.model.is_null() && tags_pass(&s.tags, input)).then(|| (so.handle.id::<class::CSceneObject>().id, s.model, s.transform))
        })
        .collect();
    let (start, end) = (crate::math::v3(input.start), crate::math::v3(input.end));
    let mut out = Vec::new();
    for (handle, model, t) in candidates {
        if !accepted(input.filter_delegate, handle) {
            continue;
        }
        let Some(m) = crate::model::get(model) else { continue };
        let a = affine(&t);
        let inv = a.inverse();
        let mut local = *input;
        (local.start, local.end) = (crate::math::from_v3(inv.transform_point3(start)), crate::math::from_v3(inv.transform_point3(end)));
        let mut hit = MeshTraceOutput::default();
        if !crate::model::trace(&m, &local, &mut hit) {
            continue;
        }
        let p = a.transform_point3(crate::math::v3(hit.position));
        hit.position = crate::math::from_v3(p);
        hit.normal = crate::math::from_v3(a.transform_vector3(crate::math::v3(hit.normal)).normalize_or_zero());
        hit.distance = p.distance(start);
        (hit.transform, hit.sceneobject_handle) = (t, handle);
        out.push(hit);
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}

fn nearest(w: scene_world::Ref, input: MeshTraceInput, output: &mut MeshTraceOutput) -> bool {
    let Some(hit) = hits(w, &input).into_iter().next() else { return false };
    *output = hit;
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, abi::slots::i_scene_world::MESH_TRACE, nearest);
    bind!(r, abi::slots::i_scene_world::MESH_TRACE_ALL, |w, input, results| {
        let hits = hits(w, &input);
        let found = !hits.is_empty();
        crate::containers::with(results, |v: &mut Vec<MeshTraceOutput>| v.extend(hits));
        found
    });
}
