//! `CModel`: models read from a compiled `.vmdl_c` ([`compiled`]) or else compiled from `.vmdl`
//! source on load (see `assets::model`), or built at
//! runtime by `CModelBuilder` ([`builder`]). Skeleton and animation queries live in `anim`.

pub mod builder;
pub mod compiled;
pub mod content;
mod draw;
mod parts;
pub mod physics;
#[cfg(test)]
pub(crate) mod tests;

use crate::resource::{self, ByName, Resource};
use abi::slots::{c_model as m, glue_resources as glue};
use abi::structs::{BBox, Vector3};
use abi::{Guid, NativeRef, Registry, StrRet, bind, class};
use assets::model as source;
use content::Content;
pub use draw::trace;
use std::ffi::CStr;
use std::sync::Arc;

pub type Ref = NativeRef<class::CModel>;

pub struct Model {
    pub name: String,
    pub content: Content,
    error: bool,
    guid: Guid,
}

impl Resource for Model {
    fn is_error(&self) -> bool {
        self.error
    }
    fn guid(&self) -> Guid {
        self.guid
    }
}

static LOADED: ByName<Model> = ByName::new();

/// The source file for a resource path managed asks for (it may name the compiled `_c`).
pub fn source_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(".vmdl") { path.to_owned() } else { format!("{path}.vmdl") }
}

/// Meshes for a model at `path`; `None` makes native's error model.
fn compile(path: &str) -> Option<Vec<source::Mesh>> {
    let file = crate::fs::resolve(&source_path(path), "GAME")?;
    let text = std::fs::read_to_string(&file).ok()?;
    let model = source::from_vmdl(&text).inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{path}: {e}"))).ok()?;
    if model.meshes.is_empty() && !model.missing.is_empty() {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("{path}: stand-in for {}", model.missing.join(", ")));
        return Some(vec![source::stand_in()]);
    }
    Some(model.meshes)
}

pub fn load(path: Option<&CStr>, guid: Guid) -> Ref {
    let name = path.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let model = LOADED.find_or_insert(&name, || {
        if let Some(m) = compiled::read(&name) {
            return Model { name: name.clone(), content: compiled::content(&m), error: false, guid };
        }
        let (meshes, error) = compile(&name).map_or_else(|| (vec![source::stand_in()], true), |m| (m, false));
        Model { name: name.clone(), content: Content::from_meshes(&meshes), error, guid }
    });
    resource::new_handle(model)
}

/// A model `CModelBuilder` made; meshless ones get the stand-in so they still draw.
pub fn create_runtime(name: &str, mut content: Content) -> Ref {
    if content.meshes.is_empty() {
        content.meshes = Content::from_meshes(&[source::stand_in()]).meshes;
    }
    resource::new_handle(Arc::new(Model { name: name.to_owned(), content, error: false, guid: Guid::default() }))
}

/// A new strong handle to the model behind `r` (null for a dead or null handle). Whoever
/// receives a handle owns it: managed wraps every returned one and destroys it when collected.
pub fn copy(r: Ref) -> Ref {
    resource::copy_handle::<_, Model>(r)
}

pub fn get(r: Ref) -> Option<Arc<Model>> {
    resource::resolve::<_, Model>(r)
}

/// A live model's resource name.
pub fn name(r: Ref) -> Option<String> {
    on(r, |m| Some(m.name.clone()))
}

/// A model's render bounds; zero for a dead handle.
pub fn bounds(r: Ref) -> BBox {
    on(r, |m| m.content.render_bounds())
}

pub(crate) fn on<R: Default>(r: Ref, f: impl FnOnce(&Model) -> R) -> R {
    get(r).map(|m| f(&m)).unwrap_or_default()
}

/// `GetMeshInfo`'s outputs: vertices, triangles, draw calls, LOD mask, translucency.
fn mesh_info(r: Ref, i: i32, out: [&mut i32; 5], lo: &mut Vector3, hi: &mut Vector3) {
    let Some((counts, b)) = on(r, |m| {
        let mm = m.content.mesh(i)?;
        let (v, t, d) = (mm.mesh.vertices().len() as i32, (mm.mesh.indices().len() / 3) as i32, mm.mesh.draw_calls().len() as i32);
        Some(([v, t, d, i32::from(mm.lod_mask), 0], mm.mesh.bounds()))
    }) else {
        return;
    };
    for (o, c) in out.into_iter().zip(counts) {
        *o = c;
    }
    (*lo, *hi) = (b.mins, b.maxs);
}

fn total_counts(m: &Model) -> (i32, i32, i32) {
    let meshes = &m.content.meshes;
    let v = meshes.iter().map(|x| x.mesh.vertices().len()).sum::<usize>() as i32;
    let t = meshes.iter().map(|x| x.mesh.indices().len() / 3).sum::<usize>() as i32;
    (v, t, meshes.iter().map(|x| x.mesh.draw_calls().len()).sum::<usize>() as i32)
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_model, Model);
    bind!(r, glue::GET_MODEL, load);
    bind!(r, glue::GET_MODEL_RESOURCE_NAME, |r| StrRet::from(name(r)));
    bind!(r, m::GET_MODEL_NAME, |r| StrRet::from(name(r)));
    bind!(r, m::GET_NUM_MESHES, |r| on(r, |m| m.content.meshes.len() as i32));
    bind!(r, m::GET_MESH_NAME, |r, i| StrRet::from(on(r, |m| m.content.mesh(i).map(|x| x.mesh.name.clone()))));
    bind!(r, m::GET_MESH_INFO, |r, i, vc, tc, draws, lods, tt, lo, hi| mesh_info(r, i, [vc, tc, draws, lods, tt], lo, hi));
    bind!(r, m::GET_TOTAL_MESH_COUNTS, |r, vertices, triangles, draws| (*vertices, *triangles, *draws) = on(r, total_counts));
    bind!(r, m::GET_MESH_BOUNDS, |r| on(r, |m| m.content.mesh_bounds()));
    bind!(r, m::GET_MODEL_RENDER_BOUNDS, bounds);
    bind!(r, m::HAS_SCENE_OBJECTS, |r| on(r, |m| !m.content.meshes.is_empty()));
    parts::register(r);
    draw::register(r);
    builder::register(r);
    physics::register(r);
}
