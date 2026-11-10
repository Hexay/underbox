//! Compiled map worlds (`g_pWorldRendererMgr`, `IWorldReference`, entity lumps). World VPKs can be
//! mounted, but nothing reads them yet, so `CreateWorld` gives null and managed's `SceneMap` logs
//! "Unable to create world" and skips the map; the per-world methods then only ever see null.
// TODO: read a map VPK's world nodes and entity lumps (vents_c) into a world reference.

use abi::slots::{c_entity_key_values as ekv, g_p_world_renderer_mgr as mgr, i_world_reference as world};
use abi::structs::Vector3;
use abi::{HandleId, NativeRef, Registry, StrRet, bind};
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{LazyLock, Mutex};

/// World name → the VPK mounted for it (`MountWorldVPK`).
static MOUNTS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn mount(name: &str, vpk: &str) -> bool {
    if !std::path::Path::new(vpk).is_file() {
        return false;
    }
    MOUNTS.lock().unwrap().insert(name.to_owned(), vpk.to_owned());
    true
}

fn unmount(name: &str) -> bool {
    MOUNTS.lock().unwrap().remove(name).is_some()
}

fn create_world(name: &str) -> NativeRef<abi::class::IWorldReference> {
    let found = MOUNTS.lock().unwrap().contains_key(name) || crate::fs::resolve(name, "GAME").is_some();
    let why = if found { "map worlds can't be loaded in this build yet" } else { "no such map" };
    abi::rt::log::warn(&format!("world '{name}': {why}"));
    NativeRef::null()
}

pub fn register(r: &mut Registry) {
    bind!(r, mgr::SERVICE_WORLD_REQUESTS, || {});
    bind!(r, mgr::UPDATE_OBJECTS_FOR_RENDERING, |_, _, _, _| 0);
    bind!(r, mgr::CREATE_WORLD, |name, _, _, _, _, _, _, _| create_world(&text(name)));
    bind!(r, mgr::MOUNT_WORLD_VPK, |name, vpk| mount(&text(name), &text(vpk)));
    bind!(r, mgr::UNMOUNT_WORLD_VPK, |name| unmount(&text(name)));

    bind!(r, world::RELEASE, |_| {});
    bind!(r, world::IS_WORLD_LOADED, |_| false);
    bind!(r, world::IS_ERROR_WORLD, |_| true);
    bind!(r, world::IS_MARKED_FOR_DELETION, |_| false);
    bind!(r, world::GET_WORLD_BOUNDS, |_, min, max| {
        (*min, *max) = (Vector3::default(), Vector3::default());
        false
    });
    bind!(r, world::GET_SCENE_WORLD, |_| HandleId::new(0));
    bind!(r, world::PRECACHE_ALL_WORLD_NODES, |_, _| {});
    bind!(r, world::GET_FOLDER, |_| StrRet::from(c""));
    bind!(r, world::GET_ENTITY_COUNT, |_, _| 0);
    bind!(r, world::GET_ENTITY_KEY_VALUES, |_, _, _| NativeRef::null());
    bind!(r, world::SET_WORLD_TRANSFORM, |_, _| {});

    // Every key is missing: managed gets its default back.
    bind!(r, ekv::GET_VALUE_STRING, |_, _, default| StrRet::from(default.map(|d| d.to_string_lossy().into_owned())));
    bind!(r, ekv::GET_VALUE_STRING_1, |_, _, default| StrRet::from(default.map(|d| d.to_string_lossy().into_owned())));
    bind!(r, ekv::GET_KEY_COUNT, |_| 0);
    bind!(r, ekv::GET_KEY, |_, _| 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_existing_vpks_and_never_creates_worlds() {
        let vpk = std::env::temp_dir().join(format!("underbox_world_{}.vpk", std::process::id()));
        std::fs::write(&vpk, b"").unwrap();
        assert!(!mount("w", "/nonexistent/x.vpk"));
        assert!(mount("w", vpk.to_str().unwrap()));
        assert!(create_world("w").is_null());
        assert!(unmount("w") && !unmount("w"));
        std::fs::remove_file(vpk).unwrap();
    }
}
