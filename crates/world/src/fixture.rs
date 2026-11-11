//! A tiny compiled map with known contents, for tests and the corpus (`corpus/mapworld`): two
//! world nodes placing dev models and a compiled crate, a KV3 entity lump, a legacy-blob child
//! lump and `world_physics`. Paths follow a Hammer compile: `maps/<name>/world.vwrld_c`,
//! `worldnodes/node00N.vwnod_c`, `entities/*.vents_c`, `world_physics.vphys_c`.

use crate::node::Matrix;
use crate::vpk::Builder;
use assets::kv3bin::Compression;
use kv::{Object, Value};

pub const PLANE: &str = "models/dev/plane.vmdl";
pub const BOX: &str = "models/dev/box.vmdl";

/// The scene objects the world nodes place: (model, transform, tint).
pub fn objects() -> Vec<(&'static str, Matrix, [f32; 4])> {
    let (c, s) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    vec![
        (PLANE, [[4.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]], [0.4, 0.5, 0.3, 1.0]),
        (BOX, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 25.0]], [0.9, 0.2, 0.2, 1.0]),
        // Second node: rotated 45 degrees about Z.
        (BOX, [[c, -s, 0.0, 60.0], [s, c, 0.0, 40.0], [0.0, 0.0, 1.0, 25.0]], [0.2, 0.3, 0.9, 1.0]),
        (COMPILED.0, [[1.0, 0.0, 0.0, COMPILED.1[0]], [0.0, 1.0, 0.0, COMPILED.1[1]], [0.0, 0.0, 1.0, 0.0]], [1.0; 4]),
    ]
}

/// The compiled crate model (with its compiled material and texture) and where it stands.
pub const COMPILED: (&str, [f32; 2]) = (assets::compiled::fixture::MODEL, [-40.0, -40.0]);

/// The map's `world_physics`: the ground plane plus a hull around the compiled crate.
pub fn world_physics() -> assets::compiled::physics::Aggregate {
    let (h, [x, y]) = (assets::compiled::fixture::HALF, COMPILED.1);
    assets::compiled::fixture::world_physics(200.0, &[([x - h, y - h, 0.0], [x + h, y + h, 2.0 * h])])
}

/// Entities of `default_ents` (KV3), each as (key, value) text.
pub fn entities() -> Vec<Vec<(&'static str, &'static str)>> {
    vec![
        vec![("classname", "worldspawn")],
        vec![("classname", "info_player_start"), ("targetname", "spawn_a"), ("origin", "0 -100 0"), ("angles", "0 90 0")],
        vec![("classname", "light_omni"), ("targetname", "lamp"), ("origin", "0 0 120"), ("color", "255 240 200 255"), ("brightness", "2"), ("range", "512"), ("enabled", "1")],
        vec![("classname", "prop_dynamic"), ("targetname", "crate"), ("model", BOX), ("origin", "-60 40 25"), ("angles", "0 30 0")],
    ]
}

/// The child lump's one entity (legacy blob, keys hashed).
pub const CHILD: [(&str, &str); 3] = [("classname", "info_target"), ("targetname", "child_target"), ("origin", "10 20 30")];

fn obj(members: Vec<(&str, Value)>) -> Value {
    Value::Object(Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

fn floats(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&f| Value::Double(f.into())).collect())
}

/// A text value as compiled KV3 types it: numbers and vectors as doubles, the rest as strings.
fn typed(key: &str, text: &str) -> Value {
    let nums: Option<Vec<f32>> = text.split(' ').map(|p| p.parse().ok()).collect();
    match nums {
        Some(n) if key != "targetname" && n.len() > 1 => floats(&n),
        Some(n) if key != "targetname" => Value::Double(n[0].into()),
        _ if key == "model" => Value::Flagged("resource".into(), Box::new(Value::String(text.into()))),
        _ => Value::String(text.into()),
    }
}

fn node(objects: &[(&str, Matrix, [f32; 4])]) -> Value {
    let so = |(model, m, tint): &(&str, Matrix, [f32; 4])| {
        obj(vec![
            ("m_renderableModel", Value::Flagged("resource".into(), Box::new(Value::String((*model).into())))),
            ("m_vTransform", Value::Array(m.iter().map(|r| floats(r)).collect())),
            ("m_vTintColor", floats(tint)),
            ("m_nObjectTypeFlags", Value::Int(0)),
            ("m_flFadeStartDistance", Value::Double(0.0)),
            ("m_flFadeEndDistance", Value::Double(0.0)),
            ("m_skin", Value::String(String::new())),
        ])
    };
    obj(vec![
        ("m_sceneObjects", Value::Array(objects.iter().map(so).collect())),
        ("m_aggregateSceneObjects", Value::Array(vec![])),
        ("m_layerNames", Value::Array(vec![Value::String("world_layer_base".into())])),
        ("m_sceneObjectLayerIndices", Value::Array(objects.iter().map(|_| Value::Int(0)).collect())),
    ])
}

/// A node with an aggregate of `model`'s two draw calls (the first moved +100 x and tinted
/// orange, the second in place) and one `model` placement forced to LOD 1.
pub fn aggregate_node(model: &str) -> Value {
    let resource = || Value::Flagged("resource".into(), Box::new(Value::String(model.into())));
    let moved: Matrix = [[1.0, 0.0, 0.0, 100.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    let mesh = |draw: i64, moved: bool, tint: [f32; 3]| {
        obj(vec![("m_nDrawCallIndex", Value::Int(draw)), ("m_bHasTransform", Value::Bool(moved)), ("m_vTintColor", floats(&tint)), ("m_objectFlags", Value::Int(0)), ("m_nLODGroupMask", Value::Int(0))])
    };
    let aggregate = obj(vec![
        ("m_renderableModel", resource()),
        ("m_aggregateMeshes", Value::Array(vec![mesh(0, true, [255.0, 128.0, 0.0]), mesh(1, false, [255.0; 3])])),
        ("m_fragmentTransforms", Value::Array(vec![Value::Array(moved.iter().map(|r| floats(r)).collect())])),
    ]);
    let forced = obj(vec![("m_renderableModel", resource()), ("m_vTransform", Value::Array(crate::node::IDENTITY.iter().map(|r| floats(r)).collect())), ("m_nLODOverride", Value::Int(1))]);
    obj(vec![("m_sceneObjects", Value::Array(vec![forced])), ("m_aggregateSceneObjects", Value::Array(vec![aggregate]))])
}

/// `maps/<name>`: one [`aggregate_node`] over the compiled fixture crate (two draw calls).
pub fn aggregate_map(name: &str) -> Builder {
    let dir = format!("maps/{name}");
    let world = obj(vec![("m_worldNodes", Value::Array(vec![obj(vec![("m_worldNodePrefix", Value::String(format!("{dir}/worldnodes/node000")))])])), ("m_entityLumps", Value::Array(vec![]))]);
    let mut b = Builder::new();
    b.add(&format!("{dir}/world.vwrld_c"), crate::resource::write(&world, Compression::None))
        .add(&format!("{dir}/worldnodes/node000.vwnod_c"), crate::resource::write(&aggregate_node(assets::compiled::fixture::MODEL), Compression::None));
    for (path, bytes) in assets::compiled::fixture::crate_files() {
        b.add(&path, bytes);
    }
    b
}

/// The legacy entity blob: version, counts, then (hash, type, value); strings are type 0x1E,
/// vectors 0x03.
fn blob(token: fn(&str) -> u32, values: &[(&str, &str)]) -> Vec<u8> {
    let mut out: Vec<u8> = [1u32, values.len() as u32, 0].iter().flat_map(|w| w.to_le_bytes()).collect();
    for (k, v) in values {
        out.extend_from_slice(&token(k).to_le_bytes());
        match v.split(' ').map(str::parse::<f32>).collect::<Result<Vec<_>, _>>() {
            Ok(n) if n.len() == 3 => {
                out.extend_from_slice(&3u32.to_le_bytes());
                n.iter().for_each(|f| out.extend_from_slice(&f.to_le_bytes()));
            }
            _ => {
                out.extend_from_slice(&0x1Eu32.to_le_bytes());
                out.extend_from_slice(v.as_bytes());
                out.push(0);
            }
        }
    }
    out
}

/// The pak of map `name` (to be written as `maps/<name>.vpk`). `token` hashes legacy entity keys
/// as `StringToken` does.
/// One leaf over ±512 split into three clusters by x quarter: 0 is x < -256, 1 is -256..256, 2 is
/// x ≥ 256. Each sees itself and its neighbours (0 and 2 can't see each other); the sky reaches
/// only cluster 2 and the sun clusters 1 and 2.
pub fn visibility() -> crate::vis::Visibility {
    let x_cells = |cells: &[u64]| (0..64u64).filter(|b| cells.contains(&(b % 4))).fold(0, |m, b| m | 1 << b);
    crate::vis::Visibility {
        clusters: 3,
        row_bytes: 1,
        min: [-512.0; 3],
        max: [512.0; 3],
        grid: 64.0,
        sky: 3,
        sun: 4,
        nodes: vec![(1, 3 | 0xFF_FFFF << 8)],
        regions: (0..3u64).map(|c| c | c << 40).collect(),
        masks: vec![x_cells(&[0]), x_cells(&[1, 2]), x_cells(&[3])],
        rows: vec![0b011, 0b111, 0b110, 0b100, 0b110],
        ..Default::default()
    }
}

/// `world.scene_c`: the map's own scene, whose game objects `MapInstance` spawns under the map.
pub const SCENE: &str = r#"{"__guid":"3c0a4f1e-5b6d-4e7f-8a9b-0c1d2e3f4a5b","GameObjects":[{"__guid":"4d1b5a2f-6c7e-4f80-9bac-1d2e3f4a5b6c","Flags":0,"Name":"scene_marker","Position":"5,6,7","Enabled":true,"Components":[]}]}"#;

pub fn tiny_map(name: &str, token: fn(&str) -> u32) -> Builder {
    let dir = format!("maps/{name}");
    let ents = format!("{dir}/entities/default_ents");
    let child = format!("{dir}/entities/default_ents_child");
    let world = obj(vec![
        (
            "m_worldNodes",
            Value::Array((0..2).map(|i| obj(vec![("m_worldNodePrefix", Value::String(format!("{dir}/worldnodes/node{i:03}")))])).collect()),
        ),
        ("m_entityLumps", Value::Array(vec![Value::Flagged("resource_name".into(), Box::new(Value::String(format!("{ents}.vents"))))])),
        ("m_builderParams", obj(vec![("m_flMinDrawVolumeSize", Value::Double(0.0))])),
    ]);
    let entity = |values: &Vec<(&str, &str)>| {
        let values = obj(values.iter().map(|(k, v)| (*k, typed(k, v))).collect());
        obj(vec![("keyValues3Data", obj(vec![("version", Value::Int(1)), ("values", values), ("attributes", obj(vec![]))])), ("m_connections", Value::Array(vec![]))])
    };
    let lump = obj(vec![
        ("m_name", Value::String("default_ents".into())),
        ("m_hammerUniqueId", Value::String(String::new())),
        ("m_childLumps", Value::Array(vec![Value::String(format!("{child}.vents"))])),
        ("m_entityKeyValues", Value::Array(entities().iter().map(entity).collect())),
    ]);
    let child_lump = obj(vec![
        ("m_name", Value::String("default_ents_child".into())),
        ("m_childLumps", Value::Array(vec![])),
        ("m_entityKeyValues", Value::Array(vec![obj(vec![("m_keyValuesData", Value::Binary(blob(token, &CHILD))), ("m_connections", Value::Array(vec![]))])])),
    ]);
    let objects = objects();
    let mut b = Builder::new();
    b.add(&format!("{dir}/world.vwrld_c"), crate::resource::write(&world, Compression::None))
        .add(&format!("{dir}/worldnodes/node000.vwnod_c"), crate::resource::write(&node(&objects[..2]), Compression::Lz4))
        .add(&format!("{dir}/worldnodes/node001.vwnod_c"), crate::resource::write(&node(&objects[2..]), Compression::Lz4))
        .add(&format!("{ents}.vents_c"), crate::resource::write(&lump, Compression::Zstd))
        .add(&format!("{child}.vents_c"), crate::resource::write(&child_lump, Compression::None))
        .add(&format!("{dir}/world_physics.vphys_c"), assets::compiled::physics::write(&world_physics()))
        .add(&format!("{dir}/world_visibility.vvis_c"), crate::vis::write(&visibility()))
        .add(&format!("{dir}/world.scene_c"), assets::game_resource::compile(SCENE, &format!("{dir}/world.scene"), 1));
    for (path, bytes) in assets::compiled::fixture::crate_files() {
        b.add(&path, bytes);
    }
    b
}
