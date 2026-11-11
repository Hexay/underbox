//! Physics data from compiled aggregates: `.vphys_c` resources (`Glue.Resources.GetPhysics`, map
//! `world_physics`) and a compiled model's PHYS block (see `assets::compiled::physics`).

use super::data::{CapsuleDesc, HullDesc, Indices, MeshDesc, Part, PhysicsData, Shapes, SphereDesc};
use super::data_slots;
use super::hull::Hull;
use crate::math::{from_v3, identity};
use abi::slots::glue_resources as glue;
use abi::structs::{Quaternion, Rotation, Transform};
use abi::{Guid, Registry, bind};
use assets::compiled::physics::{self as c, Aggregate, Matrix};
use glam::{Mat3, Vec3};
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Arc, LazyLock, Mutex};

fn transform(m: &Matrix) -> Transform {
    let basis = Mat3::from_cols_array_2d(&[[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]);
    let (scale, rotation, _) = glam::Affine3A::from_mat3(basis).to_scale_rotation_translation();
    let mut t = identity();
    t.position = from_v3(Vec3::new(m[0][3], m[1][3], m[2][3]));
    t.scale = from_v3(scale);
    t.rotation = Rotation { quat: Quaternion { x: rotation.x, y: rotation.y, z: rotation.z, w: rotation.w } };
    t
}

fn ids(i: c::Ids) -> Indices {
    Indices { surface: i.surface, attribute: i.attribute }
}

fn shapes(s: &c::Shapes) -> Shapes {
    let points = |p: &[[f32; 3]]| p.iter().copied().map(Vec3::from).collect::<Vec<_>>();
    Shapes {
        spheres: s.spheres.iter().map(|(c, r, i)| SphereDesc { center: Vec3::from(*c), radius: *r, ids: ids(*i) }).collect(),
        capsules: s.capsules.iter().map(|(a, b, r, i)| CapsuleDesc { a: Vec3::from(*a), b: Vec3::from(*b), radius: *r, ids: ids(*i) }).collect(),
        hulls: s.hulls.iter().filter_map(|(p, i)| Some(HullDesc { hull: Hull::from_points(&points(p))?, ids: ids(*i) })).collect(),
        meshes: s
            .meshes
            .iter()
            .map(|m| {
                let n = m.vertices.len() as u32;
                let triangles = m.triangles.iter().filter(|t| t.iter().all(|&v| v < n)).copied().collect();
                MeshDesc { vertices: points(&m.vertices), triangles, materials: m.materials.iter().map(|&s| s as u8).collect(), ids: ids(m.ids) }
            })
            .collect(),
    }
}

/// The engine's physics data for a compiled aggregate; surfaces resolve by name hash (unknown
/// ones fall back to "default").
pub fn data(name: &str, a: &Aggregate) -> Arc<PhysicsData> {
    let parts = a
        .parts
        .iter()
        .map(|p| Part {
            bone: p.bone.clone().unwrap_or_default(),
            bind: transform(&p.bind),
            flags: p.flags as u32,
            mass: p.mass,
            inertia_scale: p.inertia_scale,
            linear_damping: p.linear_damping,
            angular_damping: p.angular_damping,
            shapes: shapes(&p.shapes),
            ..Part::default()
        })
        .collect();
    let surfaces = a.surfaces.iter().map(|&h| crate::physics::surfaces::by_hash(h).unwrap_or(0)).collect();
    let mut d = PhysicsData::new(name, parts, Vec::new(), surfaces);
    if !a.attributes.is_empty() {
        Arc::get_mut(&mut d).expect("fresh").attributes = a.attributes.clone();
    }
    d
}

/// Loaded `.vphys_c` resources by path; `None` for a missing or unreadable file.
static LOADED: LazyLock<Mutex<HashMap<String, Option<Arc<PhysicsData>>>>> = LazyLock::new(Default::default);

fn compiled_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    let path = if path.ends_with(".vphys") { path.to_owned() } else { format!("{path}.vphys") };
    format!("{path}_c")
}

/// The physics resource at `path` (`maps/x/world_physics.vphys`, with or without `_c`).
pub fn load(path: &str) -> Option<Arc<PhysicsData>> {
    let key = compiled_path(path).to_ascii_lowercase();
    let mut cache = LOADED.lock().unwrap();
    cache
        .entry(key.clone())
        .or_insert_with(|| {
            let file = crate::fs::read(&key, "GAME")?;
            let a = c::read(&file).inspect_err(|e| abi::rt::log::warn(&format!("{path}: {e:?}"))).ok()?;
            Some(data(path.strip_suffix("_c").unwrap_or(path), &a))
        })
        .clone()
}

pub fn register(r: &mut Registry) {
    bind!(r, glue::GET_PHYSICS, |path: Option<&CStr>, _: Guid| data_slots::handle(path.and_then(|p| load(&p.to_string_lossy())).as_ref()));
}
