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
    let abi::StrRet::Owned(scene) = crate::compiled::json_from_filesystem("maps/tinyload/world.scene_c") else { panic!("world.scene_c from the pak") };
    assert_eq!(scene.to_str().unwrap(), world::fixture::SCENE, "MapInstance's map scene, read through the pak");

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

/// `world::fixture::aggregate_map`: a placement forced to LOD 1, then the aggregate's two
/// fragments as their own objects, the first moved and tinted.
#[test]
fn aggregate_fragments_and_lod_overrides_place() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("underbox_mapworld_agg_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/aggmap.vpk"), world::fixture::aggregate_map("aggmap").single()).unwrap();
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    let w = scene();
    let r = create_world("maps/aggmap.vpk", w, crate::math::identity());
    assert!(reference::get(r).is_some());
    let objects = placed(w);
    let state = |o| object::with(o, |s| (crate::model::name(s.model).unwrap_or_default(), s.transform.position.vec.x, s.tint.vec.y, s.lod));
    let crate_model = assets::compiled::fixture::MODEL;
    assert_eq!(objects.iter().map(|&o| state(o)).collect::<Vec<_>>(), [
        (crate_model.to_string(), 0.0, 1.0, Some(1)),
        (format!("{crate_model}#draw0"), 100.0, 128.0 / 255.0, None),
        (format!("{crate_model}#draw1"), 0.0, 1.0, None),
    ]);
    let draws = |o| object::with(o, |s| crate::model::get(s.model).map(|m| m.content.meshes.len()));
    assert_eq!((draws(objects[1]), draws(objects[2])), (Some(1), Some(1)), "one mesh each, one draw call");
    let mask = |o| object::with(o, |s| crate::model::get(s.model).map(|m| m.content.meshes[0].lod_mask));
    assert_eq!(mask(objects[1]), Some(crate::model::content::ALL_LODS), "a zero LOD group mask draws in every LOD");
    reference::release(r);
    reference::service();
    crate::fs::remove_search_path("GAME", dir.to_str().unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

/// `world::fixture::lod_aggregate_map`: each fragment's model draws in its own LOD groups only
/// (so LOD 0 draws just draw 0) and switches at its LOD setup's distances.
#[test]
fn aggregate_fragments_take_lod_masks_and_setup_distances() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("underbox_mapworld_lod_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/aggmap_lod.vpk"), world::fixture::lod_aggregate_map("aggmap_lod").single()).unwrap();
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    let w = scene();
    let r = create_world("maps/aggmap_lod.vpk", w, crate::math::identity());
    let lods = |o| object::with(o, |s| crate::model::get(s.model).map(|m| (m.content.meshes.iter().map(|x| x.lod_mask).collect::<Vec<_>>(), m.content.lod_distances.clone())));
    let objects = placed(w);
    assert_eq!(objects.iter().map(|&o| lods(o)).collect::<Vec<_>>(), [Some((vec![0b01], vec![0.0, 300.0])), Some((vec![0b10], vec![0.0, 300.0]))]);
    reference::release(r);
    reference::service();
    crate::fs::remove_search_path("GAME", dir.to_str().unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

static REJECT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn reject_one(handle: i32) -> u8 {
    u8::from(handle != REJECT.load(std::sync::atomic::Ordering::Relaxed))
}

/// Scene-world mesh traces over the aggregate map: the whole crate (top at z 32) and its top
/// fragment at the origin; its sides-and-bottom fragment at x 100 (open on top: the bottom is hit).
#[test]
fn scene_world_mesh_traces_hit_placed_models() {
    use super::mesh_trace::hits;
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("underbox_mapworld_trace_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/aggtrace.vpk"), world::fixture::aggregate_map("aggtrace").single()).unwrap();
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    let w = scene();
    let r = create_world("maps/aggtrace.vpk", w, crate::math::identity());
    let objects = placed(w);
    let down = |x: f32| {
        let mut i = abi::structs::MeshTraceInput::default();
        (i.start, i.end) = (crate::math::vec3(x, 0.0, 100.0), crate::math::vec3(x, 0.0, -100.0));
        i
    };
    let with = |edit: &dyn Fn(&mut abi::structs::MeshTraceInput)| {
        let mut i = down(0.0);
        edit(&mut i);
        i
    };
    let handle = |o: object::Ref| object::get(o).unwrap().handle.id::<abi::class::CSceneObject>().id;
    let summary = |input| hits(w, &input).iter().map(|h| (h.sceneobject_handle, h.position.vec.z, h.normal.vec.z)).collect::<Vec<_>>();
    assert_eq!(summary(down(100.0)), [(handle(objects[1]), 0.0, 1.0)], "the bottom, facing the ray");
    let at_origin = summary(down(0.0));
    assert_eq!(at_origin.iter().map(|h| (h.1, h.2)).collect::<Vec<_>>(), [(32.0, 1.0); 2]);
    object::with(objects[2], |s| s.tags = vec![77]);
    assert_eq!(summary(with(&|i| i.tag_exclude[0] = 77)), [(handle(objects[0]), 32.0, 1.0)]);
    assert_eq!(summary(with(&|i| i.tag_require[0] = 77)), [(handle(objects[2]), 32.0, 1.0)]);
    REJECT.store(handle(objects[0]), std::sync::atomic::Ordering::Relaxed);
    let filtered = with(&|i| i.filter_delegate = reject_one as *const () as usize);
    assert_eq!(summary(filtered), [(handle(objects[2]), 32.0, 1.0)], "the filter delegate drops a handle");
    assert!(hits(w, &down(500.0)).is_empty());
    reference::release(r);
    reference::service();
    crate::fs::remove_search_path("GAME", dir.to_str().unwrap());
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
