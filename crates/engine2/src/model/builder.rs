//! `CModelBuilder`'s meshes, mesh and material groups, LODs, physics, trace mesh, game data and
//! metadata (ModelBuilder.cs). The builder object itself belongs to `anim::builder` (bones,
//! attachments, hitboxes, animations); this side keeps its [`Content`] by builder address until
//! `CreateRuntimeModel` takes it ([`take`]) or `DeleteThis` drops it ([`forget`]).

use super::content::{self, Content, MaterialGroup, ModelMesh, TraceMesh};
use super::physics::desc;
use crate::kv3;
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh;
use abi::slots::{c_builder_material_group as mg, c_model_builder as mb};
use abi::structs::Vector3;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::{LazyLock, Mutex};

pub type Ref = NativeRef<class::CModelBuilder>;
type GroupRef = NativeRef<class::CBuilderMaterialGroup>;

#[derive(Default)]
struct Pending {
    content: Content,
    material_groups: Vec<GroupRef>,
}

static PENDING: LazyLock<Mutex<HashMap<usize, Pending>>> = LazyLock::new(Default::default);

/// LODs `SetLODSwitchDistance` may name (ModelBuilder.cs sets 0..7).
const MAX_LODS: usize = 8;

fn with<R>(b: Ref, f: impl FnOnce(&mut Pending) -> R) -> R {
    f(PENDING.lock().unwrap().entry(b.addr()).or_default())
}

fn with_content<R>(b: Ref, f: impl FnOnce(&mut Content) -> R) -> R {
    with(b, |p| f(&mut p.content))
}

fn free_groups(p: Pending) {
    p.material_groups.into_iter().for_each(|g| {
        objects::destroy(g);
    });
}

/// The builder's content for its model (the builder keeps nothing of it).
pub fn take(b: Ref) -> Content {
    let Some(mut p) = PENDING.lock().unwrap().remove(&b.addr()) else { return Content::default() };
    let groups = p.material_groups.iter().filter_map(|g| objects::get::<_, Mutex<MaterialGroup>>(*g).map(|g| g.lock().unwrap().clone()));
    p.content.material_groups = groups.collect();
    let content = std::mem::take(&mut p.content);
    free_groups(p);
    content
}

/// The builder is gone; drop what it gathered.
pub fn forget(b: Ref) {
    if let Some(p) = PENDING.lock().unwrap().remove(&b.addr()) {
        free_groups(p);
    }
}

fn add_mesh(b: Ref, mesh: render_mesh::Ref, group_mask: u64, lod_mask: i32) {
    let Some(mesh) = render_mesh::get(mesh) else { return };
    let lod_mask = u8::try_from(lod_mask).unwrap_or(content::ALL_LODS);
    with_content(b, |c| c.meshes.push(ModelMesh { mesh, group_mask, lod_mask }));
}

fn add_mesh_group(b: Ref, name: Option<&CStr>) -> i32 {
    let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    with_content(b, |c| {
        c.groups.push(name);
        c.groups.len() as i32 - 1
    })
}

fn add_material_group(b: Ref) -> GroupRef {
    let g: GroupRef = objects::create(Mutex::new(MaterialGroup::default()));
    with(b, |p| p.material_groups.push(g));
    g
}

fn on_group(g: GroupRef, f: impl FnOnce(&mut MaterialGroup)) {
    if let Some(g) = objects::get::<_, Mutex<MaterialGroup>>(g) {
        f(&mut g.lock().unwrap());
    }
}

/// Copies the tree: managed deletes its KeyValues3 right after (ModelBuilder.Metadata.cs).
fn set_key_values(b: Ref, kv: kv3::Ref) -> bool {
    let Some(node) = kv3::node::get(kv) else { return false };
    let copy = kv3::node::from_value(&kv3::to_value(&node));
    with_content(b, |c| c.game_data = Some(copy));
    true
}

fn set_lod_distance(b: Ref, lod: i32, distance: f32) {
    let Ok(lod) = usize::try_from(lod) else { return };
    if lod >= MAX_LODS {
        return;
    }
    with_content(b, |c| {
        if c.lod_distances.len() <= lod {
            c.lod_distances.resize(lod + 1, 0.0);
        }
        c.lod_distances[lod] = distance;
    });
}

fn set_trace_mesh(b: Ref, vertices: RawPtr<c_void>, nv: i32, indices: RawPtr<c_void>, ni: i32) {
    // SAFETY: managed passes pinned Vector3[nv] and int[ni] (ModelBuilder.Collision.cs).
    let (vertices, indices) = unsafe { (vertices.cast::<Vector3>().as_slice(nv.max(0) as usize), indices.cast::<i32>().as_slice(ni.max(0) as usize)) };
    let trace = TraceMesh { vertices: vertices.to_vec(), indices: indices.iter().map(|&i| i.max(0) as u32).collect() };
    with_content(b, |c| c.trace = Some(trace));
}

pub fn register(r: &mut Registry) {
    bind!(r, mb::SET_MASS, |b, mass| with_content(b, |c| c.mass = mass));
    bind!(r, mb::SET_HULL_BOUNDS, |b, lo, hi| with_content(b, |c| c.hull_bounds = content::set_bounds(lo, hi)));
    bind!(r, mb::SET_VIEW_BOUNDS, |b, lo, hi| with_content(b, |c| c.view_bounds = content::set_bounds(lo, hi)));
    bind!(r, mb::SET_EYE_POSITION, |b, v| with_content(b, |c| c.eye_position = v));
    bind!(r, mb::SET_MAX_EYE_DEFLECTION, |b, d| with_content(b, |c| c.max_eye_deflection = d));
    bind!(r, mb::SET_MODEL_KEY_VALUES, set_key_values);
    bind!(r, mb::SET_LOD_SWITCH_DISTANCE, set_lod_distance);
    bind!(r, mb::ADD_RUN_TIME_MESH, add_mesh);
    bind!(r, mb::ADD_MESH_GROUP, add_mesh_group);
    bind!(r, mb::ADD_MATERIAL_GROUP, add_material_group);
    bind!(r, mb::SET_DEFAULT_MESH_GROUP_MASK, |b, mask| with_content(b, |c| c.default_group_mask = mask));
    bind!(r, mb::SET_PHYSICS_RUNTIME, |b, a| {
        let physics = desc::build("", a);
        with_content(b, |c| c.physics = physics);
    });
    // Child resources (prefab skeletons) don't exist for runtime models.
    bind!(r, mb::APPEND_SKELETONS_FROM_CHILD_RESOURCES, |_, _| {});
    bind!(r, mb::SET_TRACE_MESH, set_trace_mesh);

    bind!(r, mg::SET_NAME, |g, name: Option<&CStr>| on_group(g, |g| g.name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    bind!(r, mg::ADD_MATERIAL, |g, m| on_group(g, |g| g.materials.extend(MaterialRef::keep(m))));
}
