//! `ISceneWorld`'s queries and links: mesh traces against its objects' models, the PVS (none:
//! every query answers "visible"), and the 3D skybox worlds and parameters managed sets.

use super::object;
use super::world::Ref;
use crate::math::{from_v3, v3};
use abi::slots::{i_scene_world as w, ipvs};
use abi::structs::{Angles, MeshTraceInput, MeshTraceOutput, Vector3};
use abi::{HandleId, NativeRef, Registry, bind, class};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::Mutex;

fn tags_pass(input: &MeshTraceInput, tags: &[u32]) -> bool {
    let set = |a: [u32; 8]| a.into_iter().filter(|t| *t != 0).collect::<Vec<_>>();
    let (require, any, exclude) = (set(input.tag_require), set(input.tag_any), set(input.tag_exclude));
    require.iter().all(|t| tags.contains(t)) && (any.is_empty() || any.iter().any(|t| tags.contains(t))) && !exclude.iter().any(|t| tags.contains(t))
}

/// Every hit of the segment against the world's model objects, each the nearest on its object.
fn hits(r: Ref, input: &MeshTraceInput) -> Vec<MeshTraceOutput> {
    let start = v3(input.start);
    let mut out = Vec::new();
    for (_, obj) in object::refs_in_worlds(&[r]) {
        let (model, transform, tags) = {
            let s = obj.state.lock().unwrap();
            (s.model, s.transform, s.tags.clone())
        };
        let Some(m) = crate::model::get(model).filter(|_| tags_pass(input, &tags)) else { continue };
        let world = crate::render::draw::model::matrix(&transform);
        let local = world.inverse();
        let to_local = |p: Vector3| from_v3(local.transform_point3(v3(p)));
        let mut local_input = *input;
        (local_input.start, local_input.end) = (to_local(input.start), to_local(input.end));
        let mut hit = MeshTraceOutput::default();
        if !crate::model::draw::trace(&m, &local_input, &mut hit) {
            continue;
        }
        let position = world.transform_point3(v3(hit.position));
        let normal = local.transpose().transform_vector3(v3(hit.normal)).normalize_or_zero();
        (hit.position, hit.normal, hit.distance) = (from_v3(position), from_v3(normal), (position - start).length());
        let id: HandleId<class::CSceneObject> = obj.handle.id();
        (hit.transform, hit.sceneobject_handle) = (transform, id.id);
        out.push(hit);
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}

#[derive(Clone, Copy, Default)]
struct Sky3d {
    origin: Vec3,
    scale: f32,
}

/// Per world: its 3D skybox worlds and parameters. TODO: draw them (see docs/renderer.md).
static SKIES: Mutex<Option<HashMap<usize, Skybox>>> = Mutex::new(None);

/// The skybox worlds' addresses and their parameters.
type Skybox = (Vec<usize>, Sky3d);

fn sky<R>(r: Ref, f: impl FnOnce(&mut Skybox) -> R) -> R {
    let mut t = SKIES.lock().unwrap();
    f(t.get_or_insert_with(HashMap::new).entry(r.addr()).or_default())
}

/// Forget a destroyed world's 3D skybox links.
pub fn forget(r: Ref) {
    if let Some(t) = SKIES.lock().unwrap().as_mut() {
        t.remove(&r.addr());
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, w::MESH_TRACE, |r, input, out: &mut MeshTraceOutput| hits(r, &input).first().map(|h| *out = *h).is_some());
    bind!(r, w::MESH_TRACE_ALL, |r, input, list| {
        let all = hits(r, &input);
        let any = !all.is_empty();
        crate::containers::with(list, |v: &mut Vec<MeshTraceOutput>| v.extend(all));
        any
    });
    // No visibility data: no PVS, and a PVS someone hands over answers "visible" everywhere.
    bind!(r, w::GET_PVS, |_| NativeRef::<class::IPVS>::NULL);
    bind!(r, w::SET_PVS, |_, _| {});
    bind!(r, ipvs::IS_EMPTY_PVS, |_| true);
    bind!(r, ipvs::IS_IN_PVS, |_, _, _, _| true);
    bind!(r, ipvs::IS_ABS_BOX_IN_PVS, |_, _, _, _, _| true);
    bind!(r, ipvs::IS_SKY_VISIBLE_FROM_POSITION, |_, _| true);
    bind!(r, ipvs::IS_SUN_VISIBLE_FROM_POSITION, |_, _| true);
    bind!(r, w::ADD3_D_SKYBOX_WORLD, |r, s: Ref| sky(r, |(worlds, _)| {
        if !worlds.contains(&s.addr()) {
            worlds.push(s.addr());
        }
    }));
    bind!(r, w::REMOVE3_D_SKYBOX_WORLD, |r, s: Ref| sky(r, |(worlds, _)| worlds.retain(|a| *a != s.addr())));
    bind!(r, w::SET3_D_SKYBOX_PARAMETERS, |r, origin, _angles: Angles, scale| sky(r, |(_, p)| *p = Sky3d { origin: v3(origin), scale }));
    bind!(r, w::GET3_D_SKYBOX_ORIGIN_AND_SCALE, |r| {
        let Sky3d { origin, scale } = sky(r, |(_, p)| *p);
        crate::math::vec4(origin.x, origin.y, origin.z, scale)
    });
}
