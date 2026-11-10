//! Compiled map worlds (`g_pWorldRendererMgr`, `IWorldReference`, `CEntityKeyValues`). Managed's
//! `SceneMap` asks for `maps/foo.vpk`; we open that pak (or one `MountWorldVPK` registered under
//! `foo`), parse it with the `world` crate and hand back a [`reference::WorldRef`]. There's no
//! vis data, so `BuildPvs` gives none and everything is drawn.

mod entity;
pub mod reference;
#[cfg(test)]
mod tests;

use abi::slots::{g_p_engine_pvs_manager as pvs, g_p_world_renderer_mgr as mgr, i_world_reference as world};
use abi::structs::{Transform, Vector3};
use abi::{HandleId, NativeRef, Registry, StrRet, bind};
use reference::get;
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// World name → the pak mounted for it (`MountWorldVPK`).
static MOUNTS: LazyLock<Mutex<HashMap<String, PathBuf>>> = LazyLock::new(Default::default);

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A pak path as given, or relative to the game search paths.
fn locate(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.is_file() { Some(p.to_owned()) } else { crate::fs::resolve(path, "GAME") }
}

fn mount(name: &str, vpk: &str) -> bool {
    let Some(path) = locate(vpk) else { return false };
    MOUNTS.lock().unwrap().insert(name.to_ascii_lowercase(), path);
    true
}

fn unmount(name: &str) -> bool {
    MOUNTS.lock().unwrap().remove(&name.to_ascii_lowercase()).is_some()
}

/// The pak for map `name`: one mounted under its file name, else `name` on the search paths.
fn pak_for(name: &str) -> Option<PathBuf> {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    MOUNTS.lock().unwrap().get(&stem).cloned().or_else(|| locate(name))
}

fn create_world(name: &str, scene: crate::scene::world::Ref, root: Transform) -> reference::Ref {
    match reference::load(name, pak_for(name), scene, &root) {
        Ok(r) => r,
        Err(why) => {
            abi::rt::log::warn(&format!("world '{name}': {why}"));
            NativeRef::null()
        }
    }
}

fn bounds(r: reference::Ref, min: &mut Vector3, max: &mut Vector3) -> bool {
    let Some(w) = get(r) else { return false };
    (*min, *max) = w.bounds;
    true
}

fn entities(r: reference::Ref, lump: Option<&CStr>) -> Vec<entity::Ref> {
    get(r).map(|w| w.entities(&text(lump))).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, mgr::SERVICE_WORLD_REQUESTS, reference::service);
    bind!(r, mgr::UPDATE_OBJECTS_FOR_RENDERING, |_, _, _, _| 0);
    bind!(r, mgr::CREATE_WORLD, |name, scene, _async, _ignore, _vis, _precache, _group, root| create_world(&text(name), scene, root));
    bind!(r, mgr::MOUNT_WORLD_VPK, |name, vpk| mount(&text(name), &text(vpk)));
    bind!(r, mgr::UNMOUNT_WORLD_VPK, |name| unmount(&text(name)));

    bind!(r, world::RELEASE, reference::release);
    // Loads finish inside CreateWorld, so a world is loaded as soon as it exists.
    bind!(r, world::IS_WORLD_LOADED, |r| get(r).is_some());
    bind!(r, world::IS_ERROR_WORLD, |r| get(r).is_none());
    bind!(r, world::IS_MARKED_FOR_DELETION, |r| get(r).is_some_and(|w| w.is_released()));
    bind!(r, world::GET_WORLD_BOUNDS, bounds);
    bind!(r, world::GET_SCENE_WORLD, |r| get(r).and_then(|w| crate::scene::world::get(w.scene_world)).map_or(HandleId::new(0), |s| s.handle.id()));
    bind!(r, world::PRECACHE_ALL_WORLD_NODES, |_, _| {});
    bind!(r, world::GET_FOLDER, |r| StrRet::from(get(r).map(|w| w.folder.clone())));
    bind!(r, world::GET_ENTITY_COUNT, |r, lump| entities(r, lump).len() as i32);
    bind!(r, world::GET_ENTITY_KEY_VALUES, |r, lump, i| usize::try_from(i).ok().and_then(|i| entities(r, lump).get(i).copied()).unwrap_or(NativeRef::null()));
    bind!(r, world::SET_WORLD_TRANSFORM, |r, t| {
        if let Some(w) = get(r) {
            w.set_transform(&t);
        }
    });
    entity::register(r);

    bind!(r, pvs::BUILD_PVS, |_| NativeRef::null());
    bind!(r, pvs::DESTROY_PVS, |_| {});
}
