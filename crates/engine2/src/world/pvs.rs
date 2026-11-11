//! `IPVS` from a map's `world_visibility.vvis_c`, and the scene-world slots that carry one
//! (`SetPVS`/`GetPVS`) or a 3D skybox. `BuildPvs` always gives a valid PVS: without vis data
//! it's empty (managed then skips its network culling) and every query answers visible.

use crate::scene::world as scene_world;
use abi::slots::{g_p_engine_pvs_manager as mgr, i_scene_world as sw, ipvs};
use abi::structs::{Angles, Vector3};
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::ffi::c_void;
use std::sync::Arc;
use world::vis::Visibility;

pub type Ref = NativeRef<class::IPVS>;

pub struct Pvs(pub Option<Visibility>);

fn get(r: Ref) -> Option<Arc<Pvs>> {
    objects::get::<_, Pvs>(r)
}

fn p(v: Vector3) -> [f32; 3] {
    crate::math::v3(v).to_array()
}

/// The PVS of the map `folder` names; empty when it has no (usable) visibility file.
pub fn build(folder: &str) -> Ref {
    let path = format!("{folder}/world_visibility.vvis_c");
    let vis = crate::fs::read(&path, "GAME").and_then(|file| match Visibility::parse(&file) {
        Ok(v) => v,
        Err(e) => {
            abi::rt::log::warn(&format!("{path}: {e}"));
            None
        }
    });
    objects::create(Pvs(vis))
}

/// The `n` points at `sources` (managed passes a `Vector3*`).
fn sources(n: i32, at: RawPtr<c_void>) -> Vec<[f32; 3]> {
    let n = usize::try_from(n).unwrap_or(0);
    if at.is_null() || n == 0 {
        return Vec::new();
    }
    // SAFETY: managed passes `n` contiguous Vector3s that outlive the call (Scene.Network).
    unsafe { std::slice::from_raw_parts(at.as_ptr().cast::<Vector3>(), n) }.iter().map(|&v| p(v)).collect()
}

fn query(r: Ref, f: impl FnOnce(&Visibility) -> bool) -> bool {
    get(r).and_then(|pvs| pvs.0.as_ref().map(f)).unwrap_or(true)
}

pub(super) fn is_empty(r: Ref) -> bool {
    get(r).is_none_or(|p| p.0.is_none())
}

pub(super) fn in_pvs(r: Ref, n: i32, at: RawPtr<c_void>, pos: Vector3) -> bool {
    query(r, |v| v.point_visible(&sources(n, at), p(pos)))
}

pub(super) fn box_in_pvs(r: Ref, n: i32, at: RawPtr<c_void>, lo: Vector3, hi: Vector3) -> bool {
    query(r, |v| v.box_visible(&sources(n, at), p(lo), p(hi)))
}

/// Whether `eye` can see the box `lo..hi` through the PVS at `r` (true without vis data).
pub fn box_visible_from(r: Ref, eye: glam::Vec3, lo: glam::Vec3, hi: glam::Vec3) -> bool {
    query(r, |v| v.box_visible(&[eye.to_array()], lo.to_array(), hi.to_array()))
}

fn on_world<R: Default>(r: scene_world::Ref, f: impl FnOnce(&scene_world::SceneWorld) -> R) -> R {
    scene_world::get(r).map(|w| f(&w)).unwrap_or_default()
}

fn set_skybox_world(r: scene_world::Ref, sky: usize) {
    on_world(r, |w| w.skybox.lock().unwrap().world = sky);
}

fn set_skybox(r: scene_world::Ref, origin: Vector3, angles: Angles, scale: f32) {
    on_world(r, |w| {
        let mut s = w.skybox.lock().unwrap();
        (s.origin, s.angles, s.scale) = (origin, angles, scale);
    });
}

/// (origin, scale) while a skybox world is set, else zero (`CameraRenderer` tests `w > 0`).
pub fn skybox_origin_and_scale(r: scene_world::Ref) -> abi::structs::Vector4 {
    let s = on_world(r, |w| *w.skybox.lock().unwrap());
    let o = crate::math::v3(s.origin);
    if s.world == 0 { crate::math::vec4(0.0, 0.0, 0.0, 0.0) } else { crate::math::vec4(o.x, o.y, o.z, s.scale) }
}

pub fn register(r: &mut Registry) {
    bind!(r, mgr::BUILD_PVS, |w| super::reference::get(w).map_or(NativeRef::null(), |w| build(&w.folder)));
    bind!(r, mgr::DESTROY_PVS, |r| {
        objects::destroy(r);
    });
    bind!(r, ipvs::IS_EMPTY_PVS, is_empty);
    bind!(r, ipvs::IS_IN_PVS, in_pvs);
    bind!(r, ipvs::IS_ABS_BOX_IN_PVS, box_in_pvs);
    bind!(r, ipvs::IS_SKY_VISIBLE_FROM_POSITION, |r, pos| query(r, |v| v.sky_visible(p(pos), false)));
    bind!(r, ipvs::IS_SUN_VISIBLE_FROM_POSITION, |r, pos| query(r, |v| v.sky_visible(p(pos), true)));

    bind!(r, sw::SET_PVS, |r, pvs: Ref| on_world(r, |w| *w.pvs.lock().unwrap() = pvs.addr()));
    bind!(r, sw::GET_PVS, |r| NativeRef::from_raw(on_world(r, |w| *w.pvs.lock().unwrap()) as *mut _));
    bind!(r, sw::ADD3_D_SKYBOX_WORLD, |r, sky: scene_world::Ref| set_skybox_world(r, sky.addr()));
    // Native clears the skybox whichever world is named (SceneWorld.Sky.cs).
    bind!(r, sw::REMOVE3_D_SKYBOX_WORLD, |r, _| set_skybox_world(r, 0));
    bind!(r, sw::SET3_D_SKYBOX_PARAMETERS, set_skybox);
    bind!(r, sw::GET3_D_SKYBOX_ORIGIN_AND_SCALE, skybox_origin_and_scale);
}
