//! Map worlds without managed: the fixture map loaded from its pak into a scene world, its
//! entities read back by token, moved, released and torn down. Also keeps the corpus copy of the
//! fixture pak current.

use super::{create_world, entity, mount, reference, unmount};
use crate::scene::{object, world as scene_world};
use abi::StringToken;
use std::path::PathBuf;
use std::sync::Arc;

fn token(s: &str) -> u32 {
    StringToken::of(s).0
}

/// A game folder holding `maps/<name>.vpk`, on the search paths.
fn game_dir(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("underbox_mapworld_{test}_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join(format!("maps/{name}.vpk")), world::fixture::tiny_map(name, token).single()).unwrap();
    dir
}

/// Freed object addresses get reused, so "gone" checks would see another test's world: run these one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn scene() -> scene_world::Ref {
    crate::handles::fake_managed();
    objects::create_shared(Arc::new(scene_world::SceneWorld::default()))
}

fn placed(w: scene_world::Ref) -> Vec<object::Ref> {
    let addrs = scene_world::get(w).unwrap().objects.lock().unwrap().clone();
    addrs.into_iter().map(|a| abi::NativeRef::from_raw(a as *mut _)).collect()
}

#[test]
fn loads_moves_and_releases_a_map() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = game_dir("load", "tinyload");
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    let w = scene();
    let r = create_world("maps/tinyload.vpk", w, crate::math::identity());
    let world = reference::get(r).expect("the fixture map loads");
    assert_eq!(world.folder, "maps/tinyload");
    assert!(world.bounds.0.vec.x < world.bounds.1.vec.x);

    let objects = placed(w);
    assert_eq!(objects.len(), world::fixture::objects().len());
    let z = |o| object::with(o, |s| s.transform.position.vec.z);
    assert_eq!(z(objects[1]), 25.0, "the red box sits at z 25");
    let tint = object::with(objects[2], |s| s.tint.vec);
    assert_eq!((tint.x, tint.z), (0.2, 0.9));

    let ents = world.entities("default_ents");
    assert_eq!(ents.len(), world::fixture::entities().len() + 1);
    for (kv, want) in ents.iter().zip(world::fixture::entities()) {
        for (k, v) in want {
            assert_eq!(entity::lookup(*kv, token(k)).as_deref(), Some(v), "{k}");
        }
    }
    for (k, v) in world::fixture::CHILD {
        assert_eq!(entity::lookup(*ents.last().unwrap(), token(k)).as_deref(), Some(v), "child {k}");
    }
    assert_eq!(world.entities("default_ents"), ents, "made once");
    assert!(crate::fs::read("maps/tinyload/world.vwrld_c", "GAME").is_some(), "the pak is mounted");

    let mut moved = crate::math::identity();
    moved.position.vec.z = 100.0;
    world.set_transform(&moved);
    assert_eq!(z(objects[1]), 125.0);

    reference::release(r);
    assert!(world.is_released() && reference::get(r).is_some(), "release waits for service");
    drop(world);
    reference::service();
    assert!(reference::get(r).is_none());
    assert!(placed(w).is_empty());
    assert!(entity::lookup(ents[0], token("classname")).is_none());
    assert!(crate::fs::read("maps/tinyload/world.vwrld_c", "GAME").is_none(), "the pak is unmounted");
    crate::fs::remove_search_path("GAME", dir.to_str().unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mounted_paks_load_by_world_name_and_missing_maps_give_null() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = game_dir("mount", "tinymount");
    let w = scene();
    assert!(create_world("maps/tinymount.vpk", w, crate::math::identity()).is_null(), "not on any search path");
    assert!(mount("tinymount", dir.join("maps/tinymount.vpk").to_str().unwrap()));
    let r = create_world("maps/tinymount.vpk", w, crate::math::identity());
    assert!(reference::get(r).is_some());
    reference::release(r);
    reference::service();
    assert!(unmount("tinymount") && !unmount("tinymount"));
    assert!(!mount("x", "/nonexistent/x.vpk"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The map's PVS answers from its vis data (see `world::fixture::visibility`); a world-less one
/// is empty and sees everything. Scene worlds keep the PVS and 3D skybox managed hands them.
#[test]
fn pvs_culls_from_the_map_visibility() {
    use super::pvs;
    use crate::math::vec3;
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = game_dir("pvs", "tinypvs");
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    let w = scene();
    let r = create_world("maps/tinypvs.vpk", w, crate::math::identity());
    let p = pvs::build(&reference::get(r).unwrap().folder);
    let mut west = [vec3(-400.0, 0.0, 0.0)];
    let at = abi::RawPtr::from_raw(west.as_mut_ptr().cast());
    assert!(!pvs::is_empty(p));
    assert!(!pvs::in_pvs(p, 1, at, vec3(400.0, 0.0, 0.0)) && pvs::in_pvs(p, 1, at, vec3(0.0, 0.0, 0.0)));
    assert!(pvs::box_in_pvs(p, 1, at, vec3(200.0, 0.0, 0.0), vec3(300.0, 1.0, 1.0)) && !pvs::box_in_pvs(p, 1, at, vec3(300.0, 0.0, 0.0), vec3(320.0, 1.0, 1.0)));
    let none = pvs::build("maps/nothing");
    assert!(pvs::is_empty(none) && pvs::in_pvs(none, 1, at, vec3(400.0, 0.0, 0.0)));

    let sky = scene();
    let world = scene_world::get(w).unwrap();
    *world.pvs.lock().unwrap() = p.addr();
    world.skybox.lock().unwrap().world = sky.addr();
    world.skybox.lock().unwrap().origin = vec3(1.0, 2.0, 3.0);
    world.skybox.lock().unwrap().scale = 16.0;
    let v = pvs::skybox_origin_and_scale(w).vec;
    assert_eq!((v.x, v.y, v.z, v.w), (1.0, 2.0, 3.0, 16.0));
    world.skybox.lock().unwrap().world = 0;
    assert_eq!(pvs::skybox_origin_and_scale(w).vec.w, 0.0, "no skybox world, no skybox");
    objects::destroy(none);
    objects::destroy(p);
    reference::release(r);
    reference::service();
    std::fs::remove_dir_all(dir).unwrap();
}

/// corpus/mapworld ships the fixture pak; `UNDERBOX_BLESS=1` rewrites it after a fixture change.
#[test]
fn corpus_pak_matches_the_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mapworld/Assets/maps/tiny.vpk");
    let want = world::fixture::tiny_map("tiny", token).single();
    if std::env::var("UNDERBOX_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    assert!(std::fs::read(&path).ok() == Some(want), "{} is stale; rerun with UNDERBOX_BLESS=1", path.display());
}
