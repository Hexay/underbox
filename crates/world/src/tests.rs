use crate::entities::Key;
use crate::vpk::Vpk;
use crate::{Error, Map, fixture, node};
use kv::{Object, Value};

/// Any hash will do here; the engine's tests use the real `StringToken`.
fn fake_token(s: &str) -> u32 {
    s.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b.into()))
}

fn open_tiny(test: &str) -> (Vpk, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("underbox_world_{test}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny.vpk");
    std::fs::write(&path, fixture::tiny_map("tiny", fake_token).single()).unwrap();
    (Vpk::open(&path).unwrap(), dir)
}

#[test]
fn loads_the_fixture_map_from_its_pak() {
    let (vpk, dir) = open_tiny("load");
    let map = Map::load("maps/tiny", &|p| vpk.read(p).and_then(Result::ok)).unwrap();
    assert!(map.warnings.is_empty(), "{:?}", map.warnings);
    let objects: Vec<_> = map.objects().map(|o| (o.model.as_str(), o.transform, o.tint)).collect();
    assert_eq!(objects, fixture::objects());
    assert_eq!(map.nodes[0].layer_names, ["world_layer_base"]);

    let ents = map.entities("default_ents");
    assert_eq!(ents.len(), fixture::entities().len() + 1, "the child lump's entity follows");
    for (got, want) in ents.iter().zip(fixture::entities()) {
        for (k, v) in want {
            assert_eq!(got.get(k), Some(v), "{k}");
        }
    }
    let child = ents.last().unwrap();
    for ((key, value), (name, want)) in child.values.iter().zip(fixture::CHILD) {
        assert_eq!((key, value.as_str()), (&Key::Hash(fake_token(name)), want));
    }
    assert_eq!(map.entities("DEFAULT_ENTS_CHILD").len(), 1, "lumps are found by file name too");
    assert!(map.entities("nope").is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_parts_warn_and_a_missing_world_fails() {
    let (vpk, dir) = open_tiny("missing");
    let without_node = |p: &str| if p.ends_with("node001.vwnod_c") { None } else { vpk.read(p).and_then(Result::ok) };
    let map = Map::load("maps/tiny", &without_node).unwrap();
    assert_eq!(map.objects().count(), 2);
    assert_eq!(map.warnings, ["maps/tiny/worldnodes/node001.vwnod_c is missing"]);
    assert!(matches!(Map::load("maps/other", &|p| vpk.read(p).and_then(Result::ok)), Err(Error::Missing(_))));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn aggregates_draw_in_world_space_and_zero_alpha_tints_are_white() {
    let o = |m: Vec<(&str, Value)>| Value::Object(Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()));
    let data = o(vec![
        ("m_sceneObjects", Value::Array(vec![o(vec![("m_renderableModel", Value::String("a.vmdl".into())), ("m_vTintColor", Value::Array(vec![Value::Double(0.5); 3].into_iter().chain([Value::Double(0.0)]).collect()))])])),
        ("m_aggregateSceneObjects", Value::Array(vec![o(vec![("m_renderableModel", Value::String("world.vmdl".into())), ("m_nLayer", Value::Int(2)), ("m_anyFlags", Value::Int(4))])])),
        ("m_clutterSceneObjects", Value::Array(vec![Value::Null])),
    ]);
    let n = node::parse(&data).unwrap();
    assert_eq!(n.objects[0].tint, [1.0; 4]);
    assert_eq!(n.objects[0].transform, node::IDENTITY);
    let agg = &n.objects[1];
    assert!(agg.aggregate && agg.model == "world.vmdl" && agg.layer == 2 && agg.flags == 4);
    assert!(agg.fragments.is_empty() && n.objects[0].lod_override == -1, "no meshes: drawn whole; no override: auto");
    assert_eq!(n.skipped, 1);
}

#[test]
fn aggregate_fragments_take_transforms_in_order_and_lod_overrides_read() {
    let n = node::parse(&fixture::aggregate_node("crate.vmdl")).unwrap();
    assert_eq!(n.objects[0].lod_override, 1);
    let f = &n.objects[1].fragments;
    let moved = [[1.0, 0.0, 0.0, 100.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    assert_eq!(f.iter().map(|f| (f.draw_call, f.transform)).collect::<Vec<_>>(), [(0, Some(moved)), (1, None)]);
    assert!((f[0].tint[1] - 128.0 / 255.0).abs() < 1e-6 && f[0].tint[2] == 0.0 && f[1].tint == [1.0; 3], "tints are 0..255");
}
