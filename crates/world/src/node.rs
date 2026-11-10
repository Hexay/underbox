//! World nodes (`.vwnod_c`): the scene objects a map draws. Each one places a model; aggregates
//! (merged world geometry) place theirs in world space. Clutter, overlays, vis clusters and
//! baked-lighting streams aren't read (research/world.md).

use crate::Error;
use crate::values::{array, field_f32, field_floats, field_str, floats, number};
use kv::{Object, Value};

/// A 3x4 affine matrix by rows: `world[i] = dot(row[i].xyz, v) + row[i].w`.
pub type Matrix = [[f32; 4]; 3];

pub const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

#[derive(Debug, Clone, PartialEq)]
pub struct SceneObject {
    /// The model resource (`m_renderableModel`, else the mesh in `m_renderable`).
    pub model: String,
    pub transform: Matrix,
    pub tint: [f32; 4],
    /// `ObjectTypeFlags`.
    pub flags: u64,
    pub fade: [f32; 2],
    pub skin: String,
    /// The layer index into [`WorldNode::layer_names`], -1 for none.
    pub layer: i32,
    /// From `m_aggregateSceneObjects`: world-space geometry, drawn untransformed.
    pub aggregate: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldNode {
    pub objects: Vec<SceneObject>,
    pub layer_names: Vec<String>,
    /// Scene objects of kinds not loaded (clutter, overlays), for diagnostics.
    pub skipped: usize,
}

fn matrix(v: Option<&Value>) -> Matrix {
    let rows: Vec<Vec<f32>> = v.and_then(Value::as_array).map(|rows| rows.iter().filter_map(floats).collect()).unwrap_or_default();
    let mut m = IDENTITY;
    if rows.len() >= 3 && rows.iter().take(3).all(|r| r.len() >= 4) {
        for (i, row) in rows.iter().take(3).enumerate() {
            m[i].copy_from_slice(&row[..4]);
        }
    }
    m
}

fn scene_object(o: &Object, layer: i32) -> Option<SceneObject> {
    let mut model = field_str(o, "m_renderableModel");
    if model.is_empty() {
        model = field_str(o, "m_renderable");
    }
    if model.is_empty() {
        return None;
    }
    let mut tint = field_floats::<4>(o, "m_vTintColor").unwrap_or([1.0; 4]);
    // A zero alpha means "no tint" in compiled worlds.
    if tint[3] == 0.0 {
        tint = [1.0; 4];
    }
    Some(SceneObject {
        model,
        transform: matrix(o.get("m_vTransform")),
        tint,
        flags: o.get("m_nObjectTypeFlags").and_then(number).unwrap_or(0.0) as u64,
        fade: [field_f32(o, "m_flFadeStartDistance").unwrap_or(0.0), field_f32(o, "m_flFadeEndDistance").unwrap_or(0.0)],
        skin: field_str(o, "m_skin"),
        layer,
        aggregate: false,
    })
}

fn aggregate(o: &Object) -> Option<SceneObject> {
    let model = field_str(o, "m_renderableModel");
    (!model.is_empty()).then(|| SceneObject {
        model,
        transform: IDENTITY,
        tint: [1.0; 4],
        flags: o.get("m_anyFlags").and_then(number).unwrap_or(0.0) as u64,
        fade: [0.0; 2],
        skin: String::new(),
        layer: o.get("m_nLayer").and_then(number).map_or(-1, |n| n as i32),
        aggregate: true,
    })
}

/// A world node's decoded DATA.
pub fn parse(data: &Value) -> Result<WorldNode, Error> {
    let o = data.as_object().ok_or_else(|| Error::Format("world node DATA isn't an object".into()))?;
    let layers: Vec<i32> = array(o, "m_sceneObjectLayerIndices").iter().map(|v| number(v).map_or(-1, |n| n as i32)).collect();
    let mut node = WorldNode::default();
    for (i, v) in array(o, "m_sceneObjects").iter().enumerate() {
        match v.as_object().and_then(|so| scene_object(so, layers.get(i).copied().unwrap_or(-1))) {
            Some(so) => node.objects.push(so),
            None => node.skipped += 1,
        }
    }
    for v in array(o, "m_aggregateSceneObjects") {
        match v.as_object().and_then(aggregate) {
            Some(so) => node.objects.push(so),
            None => node.skipped += 1,
        }
    }
    node.skipped += array(o, "m_clutterSceneObjects").len() + array(o, "m_infoOverlays").len();
    node.layer_names = array(o, "m_layerNames").iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    Ok(node)
}
