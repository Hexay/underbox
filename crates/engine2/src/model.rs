//! `CModel`: models compiled from `.vmdl` source on load (see `assets::model`).

use crate::math::vec3;
use crate::resource::{self, ByName, Resource};
use abi::slots::{c_model as m, glue_resources as glue};
use abi::structs::{BBox, Vector3};
use abi::{Guid, NativeRef, Registry, StrRet, bind, class};
use assets::model::{self as source, Mesh};
use std::ffi::CStr;

pub type Ref = NativeRef<class::CModel>;

pub struct Model {
    name: String,
    meshes: Vec<Mesh>,
    bounds: BBox,
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

fn bbox(bounds: Option<([f32; 3], [f32; 3])>) -> BBox {
    let ([a, b, c], [d, e, f]) = bounds.unwrap_or_default();
    BBox { mins: vec3(a, b, c), maxs: vec3(d, e, f) }
}

/// The source file for a resource path managed asks for (it may name the compiled `_c`).
fn source_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(".vmdl") { path.to_owned() } else { format!("{path}.vmdl") }
}

/// Meshes for a model at `path`; `None` makes native's error model.
fn compile(path: &str) -> Option<Vec<Mesh>> {
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
        let (meshes, error) = compile(&name).map_or_else(|| (vec![source::stand_in()], true), |m| (m, false));
        let bounds = bbox(source::bounds(&meshes));
        Model { name: name.clone(), meshes, bounds, error, guid }
    });
    resource::new_handle(model)
}

/// A model's render bounds; zero for a dead handle.
pub fn bounds(r: Ref) -> BBox {
    on(r, |m| m.bounds)
}

fn on<R: Default>(r: Ref, f: impl FnOnce(&Model) -> R) -> R {
    resource::resolve::<_, Model>(r).map(|m| f(&m)).unwrap_or_default()
}

fn mesh_info(r: Ref, i: i32, out: [&mut i32; 5], lo: &mut Vector3, hi: &mut Vector3) {
    let Some((vertices, triangles, b)) = on(r, |m| {
        let mesh = usize::try_from(i).ok().and_then(|i| m.meshes.get(i))?;
        let b = bbox(source::bounds(std::slice::from_ref(mesh)));
        Some((mesh.vertices.len() as i32, (mesh.indices.len() / 3) as i32, b))
    }) else {
        return;
    };
    let [vc, tc, draws, lods, translucency] = out;
    (*vc, *tc, *draws, *lods, *translucency) = (vertices, triangles, 1, 1, 0);
    (*lo, *hi) = (b.mins, b.maxs);
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_model, Model);
    bind!(r, glue::GET_MODEL, load);
    bind!(r, glue::GET_MODEL_RESOURCE_NAME, |r| StrRet::from(on(r, |m| Some(m.name.clone()))));
    bind!(r, m::GET_MODEL_NAME, |r| StrRet::from(on(r, |m| Some(m.name.clone()))));
    bind!(r, m::GET_NUM_MESHES, |r| on(r, |m| m.meshes.len() as i32));
    bind!(r, m::GET_MESH_NAME, |r, i| StrRet::from(on(r, |m| usize::try_from(i).ok().and_then(|i| m.meshes.get(i)).map(|x| x.name.clone()))));
    bind!(r, m::GET_MESH_INFO, |r, i, vc, tc, draws, lods, tt, lo, hi| mesh_info(r, i, [vc, tc, draws, lods, tt], lo, hi));
    bind!(r, m::GET_TOTAL_MESH_COUNTS, |r, vertices, triangles, draws| {
        (*vertices, *triangles, *draws) = on(r, |m| {
            let v = m.meshes.iter().map(|x| x.vertices.len()).sum::<usize>() as i32;
            let t = m.meshes.iter().map(|x| x.indices.len() / 3).sum::<usize>() as i32;
            (v, t, m.meshes.len() as i32)
        });
    });
    bind!(r, m::GET_MESH_BOUNDS, bounds);
    bind!(r, m::GET_MODEL_RENDER_BOUNDS, bounds);
    // TODO: physics from PhysicsShapeList nodes (none in the dev primitives).
    bind!(r, m::GET_PHYSICS_BOUNDS, |_| BBox::default());
    bind!(r, m::HAS_SCENE_OBJECTS, |r| on(r, |m| !m.meshes.is_empty()));
}
