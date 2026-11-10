//! Models from ModelDoc `.vmdl` source (KV3). Meshes are built from `RenderPrimitive*` nodes;
//! `RenderMeshFile` nodes name mesh files (FBX/DMX) that aren't in source control, so they're
//! reported in `missing` for the engine to stand in for.

mod primitive;

use kv::{Object, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    pub material: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub meshes: Vec<Mesh>,
    /// Mesh files the source references but this compiler can't build.
    pub missing: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Parse(kv::Error),
    NoRootNode,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "vmdl: {e:?}"),
            Error::NoRootNode => f.write_str("vmdl: no rootNode"),
        }
    }
}

/// Axis-aligned (mins, maxs) of every vertex in `meshes`; `None` without geometry.
pub fn bounds(meshes: &[Mesh]) -> Option<([f32; 3], [f32; 3])> {
    let mut points = meshes.iter().flat_map(|m| &m.vertices).map(|v| v.position);
    let first = points.next()?;
    Some(points.fold((first, first), |(lo, hi), p| ([0, 1, 2].map(|i| lo[i].min(p[i])), [0, 1, 2].map(|i| hi[i].max(p[i])))))
}

impl Model {
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        bounds(&self.meshes)
    }
}

/// What stands in for mesh files we can't build: a 16-unit cube, so the model still has
/// bounds and something to draw.
pub fn stand_in() -> Mesh {
    let mut b = primitive::Builder::default();
    primitive::cube(&mut b, [16.0; 3], [1; 3], (1.0, 1.0));
    Mesh { name: "stand_in".into(), material: String::new(), vertices: b.vertices, indices: b.indices }
}

pub fn from_vmdl(src: &str) -> Result<Model, Error> {
    let (_, root) = kv::kv3::parse(src).map_err(Error::Parse)?;
    let node = root.as_object().and_then(|o| o.get("rootNode")).and_then(Value::as_object).ok_or(Error::NoRootNode)?;
    let mut model = Model::default();
    visit(node, &mut model);
    Ok(model)
}

fn visit(node: &Object, model: &mut Model) {
    match node.str("_class").unwrap_or("") {
        "RenderPrimitiveBox" | "RenderPrimitivePlane" | "RenderPrimitiveSphere" => model.meshes.push(build(node)),
        "RenderMeshFile" => model.missing.extend(node.str("filename").map(str::to_owned)),
        _ => {}
    }
    for child in node.get("children").and_then(Value::as_array).unwrap_or_default() {
        if let Some(child) = child.as_object() {
            visit(child, model);
        }
    }
}

fn f32_of(node: &Object, key: &str, default: f32) -> f32 {
    node.get(key).and_then(Value::as_f64).map_or(default, |v| v as f32)
}

fn u32_of(node: &Object, key: &str) -> u32 {
    f32_of(node, key, 1.0).max(1.0) as u32
}

fn vec3_of(node: &Object, key: &str) -> [f32; 3] {
    let items = node.get(key).and_then(Value::as_array).unwrap_or_default();
    [0, 1, 2].map(|i| items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)
}

fn build(node: &Object) -> Mesh {
    let mut b = primitive::Builder::default();
    let max_uv = (f32_of(node, "max_u", 1.0), f32_of(node, "max_v", 1.0));
    match node.str("_class") {
        Some("RenderPrimitiveBox") => {
            let segs = [u32_of(node, "segments_x"), u32_of(node, "segments_y"), u32_of(node, "segments_z")];
            primitive::cube(&mut b, vec3_of(node, "dimensions"), segs, max_uv);
        }
        Some("RenderPrimitivePlane") => {
            let segs = (u32_of(node, "segments_x"), u32_of(node, "segments_y"));
            primitive::plane(&mut b, f32_of(node, "width", 100.0), f32_of(node, "height", 100.0), segs, max_uv);
        }
        _ => primitive::sphere(&mut b, f32_of(node, "radius", 32.0), u32_of(node, "segments_u"), u32_of(node, "segments_v")),
    }
    b.place(vec3_of(node, "origin"), vec3_of(node, "angles"));
    let name = node.str("name").unwrap_or_default().to_owned();
    Mesh { name, material: node.str("material_name").unwrap_or_default().to_owned(), vertices: b.vertices, indices: b.indices }
}

#[cfg(test)]
mod tests;
