//! `CDynamicSceneObject` (managed `SceneDynamicObject`): vertices managed rewrites as it likes
//! (DebugOverlay lines and shapes, editor gizmos), drawn as one call of its primitive type in
//! its `Material` field (render/draw/dynamic.rs).

use super::object::{self, SceneObject};
use crate::mesh::render_mesh::RenderMesh;
use crate::mesh::vertex::Vertex;
use abi::slots::c_dynamic_scene_object as dynamic;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, Weak};

type Ref = NativeRef<class::CDynamicSceneObject>;

#[derive(Default)]
struct Dynamic {
    prim: i64,
    vertices: Vec<Vertex>,
    /// Built on first draw after a change.
    mesh: Option<Arc<RenderMesh>>,
}

struct Entry {
    object: Weak<SceneObject>,
    data: Dynamic,
}

static TABLE: Mutex<Option<HashMap<usize, Entry>>> = Mutex::new(None);

fn edit(r: Ref, f: impl FnOnce(&mut Dynamic)) {
    let Some(obj) = object::get(r.cast()) else { return };
    let mut table = TABLE.lock().unwrap();
    let table = table.get_or_insert_with(HashMap::new);
    let fresh = |t: &mut HashMap<usize, Entry>| {
        t.retain(|_, e| e.object.strong_count() > 0);
        Entry { object: Arc::downgrade(&obj), data: Dynamic::default() }
    };
    let stale = table.get(&r.addr()).is_none_or(|e| !e.object.upgrade().is_some_and(|o| Arc::ptr_eq(&o, &obj)));
    if stale {
        let e = fresh(table);
        table.insert(r.addr(), e);
    }
    let d = &mut table.get_mut(&r.addr()).expect("inserted above").data;
    f(d);
    d.mesh = None;
}

/// Object `r`'s vertices as a mesh, with its managed primitive type; `None` when it has none.
pub fn mesh(r: object::Ref) -> Option<(Arc<RenderMesh>, i64)> {
    let mut table = TABLE.lock().unwrap();
    let d = &mut table.as_mut()?.get_mut(&r.addr())?.data;
    if d.vertices.is_empty() {
        return None;
    }
    let n = d.vertices.len() as u32;
    let mesh = d.mesh.get_or_insert_with(|| RenderMesh::from_vertices("dynamic", &d.vertices, &(0..n).collect::<Vec<_>>()));
    Some((mesh.clone(), d.prim))
}

/// The object's `Material` field.
pub fn material(r: object::Ref) -> abi::NativeRef<class::IMaterial2> {
    crate::render::scene::field!(get NativeRef<class::IMaterial2>, dynamic::GET_MATERIAL, r.addr())
}

fn vertex(v: &abi::structs::Vertex) -> Vertex {
    let c = v.color;
    let v4 = |x: abi::structs::Vector4| [x.vec.x, x.vec.y, x.vec.z, x.vec.w];
    Vertex {
        position: crate::math::v3(v.position).to_array(),
        color: [c.r, c.g, c.b, c.a],
        normal: crate::math::v3(v.normal).to_array(),
        tex0: v4(v.tex_coord0),
        tex1: v4(v.tex_coord1),
        tangent: v4(v.tangent),
    }
}

fn add_range(r: Ref, p: RawPtr<c_void>, n: i32) {
    if p.is_null() || n <= 0 {
        return;
    }
    // SAFETY: managed passes a pinned `Vertex[n]` (SceneDynamicObject.AddVertex).
    let src = unsafe { p.cast::<abi::structs::Vertex>().as_slice(n as usize) };
    edit(r, |d| d.vertices.extend(src.iter().map(vertex)));
}

pub fn register(r: &mut Registry) {
    // `Write`/`Init` start over with a primitive type; the count is only a capacity hint.
    bind!(r, dynamic::BEGIN, |o, prim, n| edit(o, |d| {
        d.prim = prim;
        d.vertices.clear();
        d.vertices.reserve(n.max(0) as usize);
    }));
    bind!(r, dynamic::END, |_| {});
    bind!(r, dynamic::ADD_VERTEX, |o, v| edit(o, |d| d.vertices.push(vertex(&v))));
    bind!(r, dynamic::ADD_VERTEX_RANGE, add_range);
    bind!(r, dynamic::RESET, |o| edit(o, |d| d.vertices.clear()));
}
