//! s&box's runtime animation-graph builder (`CGraphBuilder`, managed `Sandbox.Animation.GraphBuilder`).
//!
//! Real: the builder and every `C*Node::Definition` object managed fills in (fields, setters,
//! child vectors, struct elements), `Compile` (a versioned JSON snapshot of all of it, see
//! [`snapshot`]) and the two ways to make an AnimGraph2 from it (`CreateBuiltAnimGraph2`,
//! `CreateCompiledAnimGraph2`), which convert the snapshot into an `anim::graph::Definition`.
//! Stand-ins: node kinds the evaluator has no model for convert to neutral nodes (see [`convert`]).
//! Facepunch's own compiled bytes aren't our snapshot and still make no graph.

mod convert;
mod convert_more;
mod convert_pose;
mod items;
mod node_methods;
mod nodes;
pub mod snapshot;
mod vectors;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_nodes;

use abi::slots::c_graph_builder as gb;
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use items::{Item, Owned};
use snapshot::{Meta, ParamDesc};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::CGraphBuilder>;

pub struct Builder {
    owned: Arc<Owned>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    meta: Meta,
    /// Node definitions in `Add` order; a node's index is its position.
    nodes: Vec<(usize, Arc<Item>)>,
    error: Option<String>,
}

fn builder(b: Ref) -> Option<Arc<Builder>> {
    objects::get::<_, Builder>(b)
}

fn with<R: Default>(b: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    builder(b).map(|b| f(&mut b.state.lock().unwrap())).unwrap_or_default()
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn create() -> Ref {
    objects::create(Builder { owned: Arc::default(), state: Mutex::default() })
}

pub fn delete(b: Ref) {
    if let Some(b) = builder(b) {
        b.owned.free_all();
    }
    objects::destroy(b);
}

/// `InternID`: the name's string token, remembered so IDs convert back to names.
pub fn intern(b: Ref, name: &str) -> u32 {
    let id = StringToken::of(name).0;
    with(b, |s| {
        s.meta.ids.entry(id).or_insert_with(|| name.to_owned());
    });
    id
}

/// Append a resource path (`AddAnimation`, `AddReferencedGraph`): its index, or -1 with the error set.
fn add_path(b: Ref, path: Option<&CStr>, what: &str, list: fn(&mut Meta) -> &mut Vec<String>) -> i32 {
    let path = text(path);
    with(b, |s| {
        if path.trim().is_empty() {
            s.error = Some(format!("{what} path is empty"));
            return -1;
        }
        let list = list(&mut s.meta);
        list.push(path);
        list.len() as i32 - 1
    })
}

pub fn add_node<C: abi::NativeClass>(b: Ref) -> NativeRef<C> {
    let Some(bl) = builder(b) else { return NativeRef::NULL };
    let (r, item) = items::new_item::<C>(&bl.owned);
    bl.state.lock().unwrap().nodes.push((r.addr(), item));
    r
}

/// The builder's snapshot for a graph rooted at node `root`.
pub fn capture(b: Ref, root: i32, model: &str, variation: &str) -> Option<snapshot::Snapshot> {
    let bl = builder(b)?;
    let s = bl.state.lock().unwrap();
    Some(snapshot::capture(&s.meta, &s.nodes, root, model, variation))
}

pub fn register(r: &mut Registry) {
    bind!(r, gb::CREATE, create);
    bind!(r, gb::DELETE_THIS, delete);
    bind!(r, gb::GET_ERROR_STRING, |b| StrRet::from(with(b, |s| s.error.clone())));
    bind!(r, gb::COMPILE, |b, root, model, variation| match capture(b, root, &text(model), &text(variation)) {
        Some(snap) => crate::utl_buffer::create(snapshot::encode(&snap)),
        None => NativeRef::NULL,
    });
    bind!(r, gb::ADD_ANIMATION, |b, path| add_path(b, path, "animation", |m| &mut m.animations));
    bind!(r, gb::ADD_REFERENCED_GRAPH, |b, path| add_path(b, path, "referenced graph", |m| &mut m.graphs));
    bind!(r, gb::INTERN_ID, |b, name| intern(b, &text(name)));
    bind!(r, gb::SET_SKELETON, |b, path| with(b, |s| s.meta.skeleton = text(path)));
    bind!(r, gb::ADD_PERSISTENT_NODE, |b, i| with(b, |s| s.meta.persistent.push(i)));
    bind!(r, gb::ADD_CONTROL_PARAMETER, |b, name| {
        let id = intern(b, &text(name));
        with(b, |s| s.meta.control_params.push(id));
    });
    bind!(r, gb::ADD_PARAMETER_DESCRIPTION, |b, name, kind, min, max| {
        with(b, |s| s.meta.params.push(ParamDesc { name: text(name), kind, min, max, options: Vec::new() }));
    });
    bind!(r, gb::ADD_PARAMETER_OPTION, |b, option| {
        with(b, |s| {
            if let Some(p) = s.meta.params.last_mut() {
                p.options.push(text(option));
            }
        });
    });
    bind!(r, gb::ADD_VIRTUAL_PARAMETER, |b, name, i| {
        let id = intern(b, &text(name));
        with(b, |s| s.meta.virtual_params.push((id, i)));
    });
    bind!(r, gb::ADD_EXTERNAL_GRAPH_SLOT, |b, name, i| {
        let id = intern(b, &text(name));
        with(b, |s| s.meta.graph_slots.push((id, i)));
    });
    bind!(r, gb::ADD_EXTERNAL_POSE_SLOT, |b, name, i| {
        let id = intern(b, &text(name));
        with(b, |s| s.meta.pose_slots.push((id, i)));
    });
    nodes::register(r);
    node_methods::register(r);
    vectors::register(r);
}

/// The graph a snapshot describes, with the node kinds that became stand-ins logged once.
pub fn definition(name: &str, snap: &snapshot::Snapshot) -> anim::graph::Definition {
    let out = convert::convert(snap);
    if !out.unsupported.is_empty() {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("{name}: stand-ins for {}", out.unsupported.join(", ")));
    }
    out.def
}
