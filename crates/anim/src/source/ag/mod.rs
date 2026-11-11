//! AnimGraph2 sources (`.ag`, JSON written by the editor's AnimGraph tool) compiled into a
//! [`Definition`]. Nodes link by `{ "Identifier": <node guid>, "Output": <pin> }`; nested graphs
//! (groups, blend points, states, transitions) see their enclosing graphs' nodes too.
//! Unsupported node classes compile to a neutral stand-in and are listed in `unsupported`.

mod nodes;
mod nodes_more;

use crate::graph::{Definition, PoseId, ValueId};
use crate::params::{ParamDef, Target, Value};
use glam::Vec3;
use serde_json::Value as Json;
use std::collections::{BTreeSet, HashMap};

pub struct Compiled {
    pub def: Definition,
    /// Node classes compiled to a stand-in (no pose change, or a false/zero value).
    pub unsupported: Vec<String>,
}

pub fn compile(src: &str) -> Result<Compiled, serde_json::Error> {
    let root: Json = serde_json::from_str(src.trim_start_matches('\u{feff}'))?;
    let mut c = Compiler::default();
    c.def.model = str_of(&root, "Model").to_owned();
    c.def.params = root.get("parameters").and_then(Json::as_array).map(|ps| ps.iter().filter_map(param).collect()).unwrap_or_default();
    let top = Scope::new(&root, None);
    c.def.root = top.result().map(|r| c.pose_input(r.get("Pose"), &top));
    Ok(Compiled { def: c.def, unsupported: c.unsupported.into_iter().collect() })
}

fn str_of<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or_default()
}

fn f32_of(v: &Json, key: &str, default: f32) -> f32 {
    v.get(key).and_then(Json::as_f64).map_or(default, |f| f as f32)
}

fn vec3_str(s: &str) -> Vec3 {
    let mut it = s.split(',').map(|c| c.trim().parse::<f32>().unwrap_or(0.0));
    Vec3::new(it.next().unwrap_or(0.0), it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
}

fn param(p: &Json) -> Option<ParamDef> {
    let pick = |runtime: &str, authored: &str| p.get(runtime).or_else(|| p.get(authored));
    let default = match str_of(p, "Type") {
        "Float" => Value::Float(pick("RuntimeDefaultFloat", "DefaultFloat").and_then(Json::as_f64).unwrap_or(0.0) as f32),
        "Bool" => Value::Bool(pick("RuntimeDefaultBool", "DefaultBool").and_then(Json::as_bool).unwrap_or(false)),
        "ID" => Value::Id(pick("RuntimeDefaultID", "DefaultID").and_then(Json::as_str).unwrap_or_default().to_owned()),
        "Vector" => Value::Vector(vec3_str(pick("RuntimeDefaultVector", "DefaultVector").and_then(Json::as_str).unwrap_or_default())),
        "Target" => Value::Target(Target::default()),
        _ => return None,
    };
    let options = p.get("ExpectedValues").and_then(Json::as_array).map(|o| o.iter().filter_map(Json::as_str).map(str::to_owned).collect()).unwrap_or_default();
    let (min, max) = (f32_of(p, "Min", 0.0), f32_of(p, "Max", 0.0));
    Some(ParamDef { name: str_of(p, "Name").to_owned(), default, min, max, options, auto_reset: false })
}

/// One graph's nodes by identifier, and the graph it's nested in.
struct Scope<'a> {
    nodes: HashMap<&'a str, &'a Json>,
    list: &'a [Json],
    parent: Option<&'a Scope<'a>>,
}

impl<'a> Scope<'a> {
    /// `graph` is a `{ "Version", "nodes": [...] }` object.
    fn new(graph: &'a Json, parent: Option<&'a Scope<'a>>) -> Self {
        let list = graph.get("nodes").and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default();
        let nodes = list.iter().filter_map(|n| Some((n.get("Identifier")?.as_str()?, n))).collect();
        Self { nodes, list, parent }
    }

    fn find(&self, id: &str) -> Option<&'a Json> {
        self.nodes.get(id).copied().or_else(|| self.parent?.find(id))
    }

    fn of_class(&self, class: &str) -> Option<&'a Json> {
        self.list.iter().find(|n| str_of(n, "_class") == class)
    }

    fn result(&self) -> Option<&'a Json> {
        self.of_class("PoseResultNode")
    }
}

/// The node a `{ "Identifier", "Output" }` link points at.
fn link<'a>(scope: &Scope<'a>, l: Option<&Json>) -> Option<(&'a Json, String)> {
    let id = l?.get("Identifier")?.as_str()?;
    Some((scope.find(id)?, id.to_owned()))
}

#[derive(Default)]
struct Compiler {
    def: Definition,
    values: HashMap<String, ValueId>,
    poses: HashMap<String, PoseId>,
    unsupported: BTreeSet<String>,
}

#[cfg(test)]
mod tests;
