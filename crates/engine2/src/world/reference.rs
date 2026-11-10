//! `IWorldReference`: a loaded map. Loading mounts the map's pak, places every world-node scene
//! object in the caller's scene world (through the model path, like managed's own scene models)
//! and keeps the entity lumps for managed's `MapLoader`. Release queues teardown for
//! `ServiceWorldRequests`, as native's does.

use super::entity;
use crate::handles::Kind;
use crate::scene::{object, world as scene_world};
use abi::structs::{BBox, Quaternion, Rotation, Transform, Vector3};
use abi::{NativeRef, class};
use glam::{Affine3A, Mat3, Vec3};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use world::vpk::Vpk;

pub type Ref = NativeRef<class::IWorldReference>;

pub struct WorldRef {
    pub folder: String,
    map: world::Map,
    pak: Option<Arc<Vpk>>,
    pub scene_world: scene_world::Ref,
    /// Each placed object and its transform within the map.
    objects: Mutex<Vec<(object::Ref, Affine3A)>>,
    /// Entity key values per lump name, made on first request.
    entities: Mutex<HashMap<String, Vec<entity::Ref>>>,
    pub bounds: (Vector3, Vector3),
    released: AtomicBool,
}

/// Released worlds waiting for `ServiceWorldRequests`.
static RELEASED: LazyLock<Mutex<Vec<Ref>>> = LazyLock::new(Default::default);

pub fn get(r: Ref) -> Option<Arc<WorldRef>> {
    objects::get::<_, WorldRef>(r)
}

/// `maps/foo.vpk` → `maps/foo`, the folder its files live under.
pub fn folder_of(name: &str) -> String {
    let name = name.replace('\\', "/");
    let name = name.trim_start_matches(['/', ' ']);
    name.strip_suffix(".vpk").unwrap_or(name).to_owned()
}

fn affine(t: &Transform) -> Affine3A {
    let q = crate::math::quat(t.rotation);
    Affine3A::from_scale_rotation_translation(crate::math::v3(t.scale), q, crate::math::v3(t.position))
}

fn transform(a: Affine3A) -> Transform {
    let (s, q, p) = a.to_scale_rotation_translation();
    Transform { position: crate::math::from_v3(p), scale: crate::math::from_v3(s), rotation: Rotation { quat: Quaternion { x: q.x, y: q.y, z: q.z, w: q.w } } }
}

fn local(m: &world::node::Matrix) -> Affine3A {
    let col = |j: usize| Vec3::new(m[0][j], m[1][j], m[2][j]);
    Affine3A::from_mat3_translation(Mat3::from_cols(col(0), col(1), col(2)), col(3))
}

/// `b` moved by `a`: the box around its eight transformed corners.
fn moved(a: Affine3A, b: BBox) -> (Vec3, Vec3) {
    let (lo, hi) = (crate::math::v3(b.mins), crate::math::v3(b.maxs));
    (0..8).map(|i| a.transform_point3(Vec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][i >> 2]))).fold((Vec3::MAX, Vec3::MIN), |(l, h), p| (l.min(p), h.max(p)))
}

/// Load the map `name` names (`maps/foo.vpk`) from `pak` (or loose files when `None`) into `scene`.
pub fn load(name: &str, pak: Option<PathBuf>, scene: scene_world::Ref, root: &Transform) -> Result<Ref, String> {
    let pak = pak.map(|p| Vpk::open(&p).map(Arc::new).map_err(|e| format!("{}: {e}", p.display()))).transpose()?;
    if let Some(p) = &pak {
        crate::fs::mount_pak(p.clone());
    }
    let folder = folder_of(name);
    let map = match world::Map::load(&folder, &|path| crate::fs::read(path, "GAME")) {
        Ok(map) => map,
        Err(e) => {
            if let Some(p) = &pak {
                crate::fs::unmount_pak(p);
            }
            return Err(e.to_string());
        }
    };
    for w in &map.warnings {
        abi::rt::log::warn(&format!("world '{name}': {w}"));
    }
    let root = affine(root);
    let mut placed = Vec::new();
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    for so in map.objects() {
        let model_name = std::ffi::CString::new(so.model.as_str()).unwrap_or_default();
        let model = crate::model::load(Some(&model_name), abi::Guid::default());
        let at = local(&so.transform);
        let bounds = crate::model::bounds(model);
        let (l, h) = moved(root * at, bounds);
        (lo, hi) = (lo.min(l), hi.max(h));
        let tint = crate::math::vec4(so.tint[0], so.tint[1], so.tint[2], so.tint[3]);
        let state = object::State { model, transform: transform(root * at), bounds, tint, cull_distance: so.fade[1], ..object::State::default() };
        let (r, _) = object::create_native::<class::CSceneObject, class::CSceneObject>(scene, Kind::SceneObject, state, None);
        placed.push((r, at));
    }
    if placed.is_empty() {
        (lo, hi) = (Vec3::ZERO, Vec3::ZERO);
    }
    let bounds = (crate::math::from_v3(lo), crate::math::from_v3(hi));
    let w = WorldRef { folder, map, pak, scene_world: scene, objects: Mutex::new(placed), entities: Mutex::default(), bounds, released: AtomicBool::new(false) };
    Ok(objects::create(w))
}

impl WorldRef {
    /// Lump `lump`'s entities (children included) as native key values, made once.
    pub fn entities(&self, lump: &str) -> Vec<entity::Ref> {
        let mut cache = self.entities.lock().unwrap();
        cache.entry(lump.to_ascii_lowercase()).or_insert_with(|| self.map.entities(lump).into_iter().map(entity::create).collect()).clone()
    }

    pub fn set_transform(&self, root: &Transform) {
        let root = affine(root);
        for (r, at) in self.objects.lock().unwrap().iter() {
            object::with(*r, |s| s.transform = transform(root * *at));
        }
    }

    pub fn is_released(&self) -> bool {
        self.released.load(Ordering::Relaxed)
    }
}

pub fn release(r: Ref) {
    if let Some(w) = get(r)
        && !w.released.swap(true, Ordering::Relaxed)
    {
        RELEASED.lock().unwrap().push(r);
    }
}

/// Tear down released worlds: their scene objects, entity values and pak mount.
pub fn service() {
    let released = std::mem::take(&mut *RELEASED.lock().unwrap());
    for r in released {
        let Some(w) = get(r) else { continue };
        for (o, _) in w.objects.lock().unwrap().drain(..) {
            object::delete(o);
        }
        for kv in w.entities.lock().unwrap().drain().flat_map(|(_, v)| v) {
            objects::destroy(kv);
        }
        if let Some(p) = &w.pak {
            crate::fs::unmount_pak(p);
        }
        objects::destroy(r);
    }
}
