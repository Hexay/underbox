//! What `CGraphBuilder::Compile` writes: a versioned JSON snapshot of the builder (graph-level
//! data, then every node with its fields, setter values and child vectors), behind a magic
//! prefix so `CreateCompiledAnimGraph2` can tell it from Facepunch's own format.

use super::items::{Child as LiveChild, Item};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::Arc;

const MAGIC: &[u8] = b"underbox-animgraph2\n";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamDesc {
    pub name: String,
    /// `GraphValueKind`: Unknown, Bool, Id, Float, Vector, Target, BoneMask, Pose.
    pub kind: i64,
    pub min: f32,
    pub max: f32,
    pub options: Vec<String>,
}

/// The builder's graph-level data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub skeleton: String,
    pub animations: Vec<String>,
    pub graphs: Vec<String>,
    pub persistent: Vec<i32>,
    /// Control parameter IDs, in the order of the control-parameter nodes.
    pub control_params: Vec<u32>,
    pub params: Vec<ParamDesc>,
    /// (ID, node index) pairs.
    pub virtual_params: Vec<(u32, i32)>,
    pub graph_slots: Vec<(u32, i32)>,
    pub pose_slots: Vec<(u32, i32)>,
    /// Every interned ID's name.
    pub ids: BTreeMap<u32, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// The native class, e.g. `CClipNode_Definition`.
    pub class: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Json>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub setters: BTreeMap<String, i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub children: BTreeMap<String, Child>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Child {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ints: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floats: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vec2: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Node>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<Box<Node>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub root: i32,
    pub model: String,
    pub variation: String,
    pub meta: Meta,
    pub nodes: Vec<Node>,
}

/// `Get__CClipNode_Definition_m_dataSlotIdx` → `m_dataSlotIdx`.
fn field_name(getter: &str) -> Option<&str> {
    let rest = getter.strip_prefix("Get__")?;
    rest.find("_m_").map(|i| &rest[i + 1..])
}

fn node(addr: usize, item: &Item) -> Node {
    use abi::rt::fields::Dumped;
    let fields = abi::rt::fields::dump(addr)
        .into_iter()
        .filter_map(|(key, v)| {
            let name = field_name(abi::NAMES.get(key)?)?.to_owned();
            let v = match v {
                Dumped::Bool(b) => Json::from(b),
                Dumped::Int(i) => Json::from(i),
                Dumped::Float(f) => Json::from(f),
                Dumped::Floats(f) => Json::from(f),
                Dumped::Str(s) => Json::from(s),
            };
            Some((name, v))
        })
        .collect();
    let setters = item.setters.lock().unwrap().iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let children = item.children.lock().unwrap().iter().map(|(k, c)| (k.to_string(), child(c))).collect();
    Node { class: item.class.to_owned(), fields, setters, children }
}

fn child(c: &LiveChild) -> Child {
    match c {
        LiveChild::List(_, l) => Child {
            ints: l.ints.lock().unwrap().clone(),
            vec2: l.vec2.lock().unwrap().clone(),
            items: l.items.lock().unwrap().iter().map(|(a, i)| node(*a, i)).collect(),
            ..Child::default()
        },
        LiveChild::Floats(_, v) => Child { floats: v.0.lock().unwrap().clone(), ..Child::default() },
        LiveChild::Object(a, i) => Child { object: Some(Box::new(node(*a, i))), ..Child::default() },
    }
}

pub fn capture(meta: &Meta, nodes: &[(usize, Arc<Item>)], root: i32, model: &str, variation: &str) -> Snapshot {
    Snapshot {
        version: VERSION,
        root,
        model: model.to_owned(),
        variation: variation.to_owned(),
        meta: meta.clone(),
        nodes: nodes.iter().map(|(a, i)| node(*a, i)).collect(),
    }
}

pub fn encode(s: &Snapshot) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend(serde_json::to_vec(s).expect("snapshot serializes"));
    out
}

/// The snapshot in `bytes`, if they're ours and of a version we read.
pub fn decode(bytes: &[u8]) -> Option<Snapshot> {
    let s: Snapshot = serde_json::from_slice(bytes.strip_prefix(MAGIC)?).ok()?;
    (s.version == VERSION).then_some(s)
}

impl Node {
    pub fn field(&self, name: &str) -> Option<&Json> {
        self.fields.get(name)
    }

    /// A node-index field; unset reads as -1 (native's invalid index).
    pub fn idx(&self, name: &str) -> i64 {
        self.field(name).and_then(Json::as_i64).unwrap_or(-1)
    }

    pub fn float(&self, name: &str) -> f32 {
        self.field(name).and_then(Json::as_f64).unwrap_or(0.0) as f32
    }

    pub fn bool(&self, name: &str) -> bool {
        self.field(name).and_then(Json::as_bool).unwrap_or(false)
    }

    pub fn setter(&self, name: &str) -> i64 {
        self.setters.get(name).copied().unwrap_or(0)
    }

    pub fn ints(&self, name: &str) -> &[i64] {
        self.children.get(name).map_or(&[], |c| &c.ints)
    }

    pub fn floats(&self, name: &str) -> &[f32] {
        self.children.get(name).map_or(&[], |c| &c.floats)
    }

    pub fn items(&self, name: &str) -> &[Node] {
        self.children.get(name).map_or(&[], |c| &c.items)
    }
}
