//! A compiled mesh (`CRenderMesh` data: a `.vmesh_c` DATA or a model's MDAT block) with its
//! buffers, flattened to one vertex list and one index list. Draw calls keep their material and
//! index range; base vertices and multi-stream bindings are resolved here.

use super::kv::{self, float_or, floats, int, int_value, list, obj, objects, resource, vec3};
use super::vbib::{self, Attribute, Buffer, Buffers};
use super::vertex::{self, Vertex, dxgi};
use super::{Error, format};
use ::kv::{Object, Value};
use std::collections::HashMap;

/// `RENDER_PRIM_TRIANGLES`.
pub const TRIANGLES: i32 = 5;

/// `RenderPrimitiveType_t` in its order, up to the patch lists.
const PRIMITIVES: [&str; 11] = [
    "RENDER_PRIM_POINTS",
    "RENDER_PRIM_LINES",
    "RENDER_PRIM_LINES_WITH_ADJACENCY",
    "RENDER_PRIM_LINE_STRIP",
    "RENDER_PRIM_LINE_STRIP_WITH_ADJACENCY",
    "RENDER_PRIM_TRIANGLES",
    "RENDER_PRIM_TRIANGLES_WITH_ADJACENCY",
    "RENDER_PRIM_TRIANGLE_STRIP",
    "RENDER_PRIM_TRIANGLE_STRIP_WITH_ADJACENCY",
    "RENDER_PRIM_INSTANCED_QUADS",
    "RENDER_PRIM_HETEROGENOUS",
];

/// Compiled files spell the enum by name; older fixtures wrote its value.
fn primitive(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::String(s)) => PRIMITIVES.iter().position(|p| p == s).map_or(TRIANGLES, |i| i as i32),
        Some(v) => kv::int_of(v).unwrap_or(TRIANGLES as i64) as i32,
        None => TRIANGLES,
    }
}

fn primitive_name(p: i32) -> &'static str {
    usize::try_from(p).ok().and_then(|i| PRIMITIVES.get(i)).copied().unwrap_or(PRIMITIVES[TRIANGLES as usize])
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawCall {
    /// Material resource path (`materials/x.vmat`).
    pub material: String,
    pub primitive: i32,
    /// Range in [`Mesh::indices`].
    pub start_index: u32,
    pub index_count: u32,
    pub tint: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub draws: Vec<DrawCall>,
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

fn handle(o: &Object) -> i64 {
    int(o, "m_hBuffer")
}

/// The mesh in `data` (its `m_sceneObjects`), drawing from `buffers`.
pub fn build(data: &Object, buffers: &Buffers) -> Result<Mesh, Error> {
    let mut mesh = Mesh::default();
    // Vertex buffer sets already flattened, and where they start in `mesh.vertices`.
    let mut placed: HashMap<Vec<i64>, u32> = HashMap::new();
    for so in objects(data, "m_sceneObjects") {
        let (lo, hi) = (vec3(so, "m_vMinBounds"), vec3(so, "m_vMaxBounds"));
        mesh.bounds = Some(mesh.bounds.map_or((lo, hi), |(a, b)| (std::array::from_fn(|i| a[i].min(lo[i])), std::array::from_fn(|i| b[i].max(hi[i])))));
        for dc in objects(so, "m_drawCalls") {
            let streams: Vec<i64> = objects(dc, "m_vertexBuffers").map(handle).collect();
            let base_set = match placed.get(&streams) {
                Some(&b) => b,
                None => {
                    let bound = streams.iter().map(|&h| buffers.vertex.get(h as usize)).collect::<Option<Vec<&Buffer>>>().ok_or_else(|| format("draw call names a missing vertex buffer"))?;
                    let b = mesh.vertices.len() as u32;
                    mesh.vertices.extend(vertex::decode(&bound));
                    placed.insert(streams, b);
                    b
                }
            };
            let base = base_set + int(dc, "m_nBaseVertex") as u32;
            let (start, count) = (int(dc, "m_nStartIndex") as usize, int(dc, "m_nIndexCount") as usize);
            let indices: Vec<u32> = match buffers.index.get(handle(kv::object(dc, "m_indexBuffer")) as usize) {
                Some(ib) if count > 0 => vbib::indices_of(ib).get(start..start + count).ok_or_else(|| format("draw call indices out of range"))?.iter().map(|i| i + base).collect(),
                _ => (0..int(dc, "m_nVertexCount") as u32).map(|i| i + base).collect(),
            };
            let material = dc.get("m_material").or_else(|| dc.get("m_pMaterial")).and_then(Value::as_str).unwrap_or_default().to_owned();
            let [r, g, b] = dc.get("m_vTintColor").map_or([1.0; 3], |v| kv::floats_of(Some(v)));
            let start_index = mesh.indices.len() as u32;
            mesh.indices.extend(&indices);
            let primitive = primitive(dc.get("m_nPrimitiveType"));
            mesh.draws.push(DrawCall { material, primitive, start_index, index_count: indices.len() as u32, tint: [r, g, b, float_or(dc, "m_flAlpha", 1.0)] });
        }
    }
    Ok(mesh)
}

/// The layout [`write_buffers`] stores vertices in: floats except unorm colour, byte bone
/// indices and unorm weights (real skins only with those), nothing compressed.
fn layout() -> Vec<Attribute> {
    let field = |semantic: &str, index, format, offset| Attribute { semantic: semantic.into(), index, format, offset, slot: 0 };
    vec![
        field("POSITION", 0, dxgi::R32G32B32_FLOAT, 0),
        field("NORMAL", 0, dxgi::R32G32B32_FLOAT, 12),
        field("TANGENT", 0, dxgi::R32G32B32A32_FLOAT, 24),
        field("TEXCOORD", 0, dxgi::R32G32_FLOAT, 40),
        field("TEXCOORD", 1, dxgi::R32G32_FLOAT, 48),
        field("COLOR", 0, dxgi::R8G8B8A8_UNORM, 56),
        field("BLENDINDICES", 0, dxgi::R8G8B8A8_UINT, 60),
        field("BLENDWEIGHT", 0, dxgi::R8G8B8A8_UNORM, 64),
    ]
}

const STRIDE: usize = 68;

/// `w` as unorm bytes summing to 255, the rounding error on the largest, as real's compiler stores them.
pub fn unorm_weights(w: [f32; 4]) -> [u8; 4] {
    let total: f32 = w.iter().sum();
    let scale = if total > 0.0 { 255.0 / total } else { 0.0 };
    let mut out = w.map(|x| (x * scale).round().clamp(0.0, 255.0) as u8);
    let sum: i32 = out.iter().map(|x| i32::from(*x)).sum();
    if sum > 0 {
        let big = (0..4).max_by_key(|&i| out[i]).unwrap_or(0);
        out[big] = (i32::from(out[big]) + 255 - sum).clamp(0, 255) as u8;
    }
    out
}

/// One vertex buffer and one 32-bit index buffer holding `mesh`.
pub fn write_buffers(mesh: &Mesh) -> Buffers {
    let mut data = Vec::with_capacity(mesh.vertices.len() * STRIDE);
    for v in &mesh.vertices {
        let floats = v.position.iter().chain(&v.normal).chain(&v.tangent).chain(v.uv.iter().flatten());
        floats.for_each(|f| data.extend(f.to_le_bytes()));
        data.extend(v.color);
        data.extend(v.bones);
        data.extend(unorm_weights(v.weights));
    }
    let vb = Buffer { count: mesh.vertices.len(), stride: STRIDE, attributes: layout(), data };
    let ib = Buffer { count: mesh.indices.len(), stride: 4, attributes: Vec::new(), data: mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect() };
    Buffers { vertex: vec![vb], index: vec![ib] }
}

/// The DATA object for `mesh` over [`write_buffers`]: one scene object, a draw call per draw.
pub fn write_data(mesh: &Mesh) -> Value {
    let (lo, hi) = mesh.bounds.unwrap_or_default();
    let draw = |d: &DrawCall| {
        obj(vec![
            ("m_nPrimitiveType", Value::String(primitive_name(d.primitive).into())),
            ("m_nBaseVertex", int_value(0)),
            ("m_nVertexCount", int_value(mesh.vertices.len() as i64)),
            ("m_nStartIndex", int_value(d.start_index)),
            ("m_nIndexCount", int_value(d.index_count)),
            ("m_nStartInstance", int_value(0)),
            ("m_nInstanceCount", int_value(0)),
            ("m_flUvDensity", Value::Double(1.0)),
            ("m_vTintColor", floats(&d.tint[..3])),
            ("m_flAlpha", Value::Double(d.tint[3].into())),
            ("m_CullDataIndex", int_value(-1)),
            ("m_nMeshID", int_value(0)),
            ("m_indexBuffer", obj(vec![("m_hBuffer", int_value(0)), ("m_nBindOffsetBytes", int_value(0))])),
            ("m_material", resource(&d.material)),
            ("m_vertexBuffers", list([obj(vec![("m_hBuffer", int_value(0)), ("m_nBindOffsetBytes", int_value(0))])])),
            ("m_bUseCompressedNormalTangent", Value::Bool(false)),
        ])
    };
    let scene_object = obj(vec![
        ("m_vMinBounds", floats(&lo)),
        ("m_vMaxBounds", floats(&hi)),
        ("m_drawCalls", list(mesh.draws.iter().map(draw))),
        ("m_drawCullData", list([])),
        ("m_vTintColor", floats(&[0.0; 4])),
    ]);
    // `CRenderMesh` as real's compiler writes it; the skeleton is the model's (see model_write).
    obj(vec![
        ("_class", Value::String("CRenderMesh".into())),
        ("m_sceneObjects", list([scene_object])),
        ("m_constraints", list([])),
        ("m_skeleton", obj(vec![("m_bones", list([])), ("m_boneParents", list([])), ("m_nBoneWeightCount", int_value(4))])),
        ("traceDataAllocated", int_value(0)),
        ("m_attachments", list([])),
        ("m_hitboxsets", list([])),
        ("m_morphSet", Value::Null),
    ])
}

/// A standalone `.vmesh_c`: DATA and a VBIB block.
pub fn write(mesh: &Mesh, packing: vbib::Packing) -> Vec<u8> {
    let refs: Vec<&str> = mesh.draws.iter().map(|d| d.material.as_str()).collect();
    super::write(0, &refs, vec![(*b"DATA", super::kv3(&write_data(mesh))), (*b"VBIB", vbib::write(&write_buffers(mesh), packing))])
}

/// A standalone `.vmesh_c`: buffers from its VBIB block, else KV3 buffers in DATA.
pub fn read(file: &[u8]) -> Result<Mesh, Error> {
    read_with_data(file).map(|(m, _)| m)
}

/// As [`read`], with the DATA object (attachments, hitboxes and the morph set live there).
pub fn read_with_data(file: &[u8]) -> Result<(Mesh, Object), Error> {
    let Value::Object(data) = super::kv3_block(file, b"DATA", "DATA")? else { return Err(format("mesh DATA isn't an object")) };
    let buffers = match crate::container::block(file, b"VBIB")? {
        Some(b) => vbib::read(b)?,
        None => vbib::from_kv3(&data, file)?,
    };
    Ok((build(&data, &buffers)?, data))
}
