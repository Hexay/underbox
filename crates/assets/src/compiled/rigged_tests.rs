use super::fixture::{self, RIGGED, SMILE_LIFT};
use super::kv::{floats, int_value, list, obj};
use super::model::{self, Model};
use ::kv::Value;

fn rigged() -> Model {
    let files = fixture::rigged_files();
    let load = |p: &str| files.iter().find(|(n, _)| n == p).map(|(_, b)| b.clone());
    model::read(&load(&format!("{RIGGED}_c")).unwrap(), &load).unwrap()
}

#[test]
fn rigged_model_reads_animations_from_blocks_and_groups() {
    let m = rigged();
    assert!(m.missing.is_empty(), "{:?}", m.missing);
    let names: Vec<&str> = m.animations.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["wave", "nod", "stretch"]);
    let wave = &m.animations[0];
    assert!(wave.looping && !wave.delta && wave.fps == 10.0);
    assert_eq!(wave.morphs.iter().map(|w| w[0]).collect::<Vec<_>>(), [0.0, 0.25, 0.5, 0.75, 1.0]);
    let head = wave.frames[4][2];
    assert!((head.position[2] - (fixture::rig_bones()[2].position[2] + 2.0)).abs() < 1e-2 && (head.scale - 2.0).abs() < 1e-6, "{head:?}");
    assert_eq!(wave.events, [fixture::footstep()], "m_eventArray with its KV3 payload");
    assert!(m.animations[1].delta && m.animations[1].events.is_empty());
    assert_eq!(m.animations[2].frames[2][0].position, [2.0, 0.0, 0.0]);
}

#[test]
fn ragdoll_joints_round_trip_and_compounds_flatten() {
    assert_eq!(rigged().physics, Some(fixture::ragdoll()));
    let capsule = obj(vec![("m_vCenter", list([floats(&[0.0; 3]), floats(&[0.0, 0.0, 4.0])])), ("m_flRadius", Value::Double(1.0))]);
    let sphere = obj(vec![("m_vCenter", floats(&[1.0, 2.0, 3.0])), ("m_flRadius", Value::Double(2.0))]);
    let compound = obj(vec![("m_Capsules", list([capsule.clone()])), ("m_Spheres", list([sphere]))]);
    let desc = obj(vec![("m_nSurfacePropertyIndex", int_value(3)), ("m_nCollisionAttributeIndex", int_value(1)), ("m_Compound", compound)]);
    let own = obj(vec![("m_Capsule", capsule.clone())]);
    let part = obj(vec![("m_rnShape", obj(vec![("m_capsules", list([own])), ("m_compounds", list([desc]))]))]);
    let Value::Object(agg) = obj(vec![("m_parts", list([part]))]) else { unreachable!() };
    let s = &super::physics::from_kv3(&agg).parts[0].shapes;
    assert_eq!(s.capsules.len(), 2, "the part's own capsule plus the compound's");
    assert_eq!((s.capsules[1].3.surface, s.capsules[1].3.attribute), (3, 1), "compound children take its ids");
    assert_eq!((s.spheres[0].0, s.spheres[0].1), ([1.0, 2.0, 3.0], 2.0));

    // Per-child surfaces: ids run spheres first, then capsules.
    let compound = obj(vec![("m_Capsules", list([capsule.clone()])), ("m_Spheres", list([obj(vec![("m_flRadius", Value::Double(1.0))])])), ("m_CompoundMaterialIndices", Value::Binary(vec![5, 7]))]);
    let desc = obj(vec![("m_nSurfacePropertyIndex", int_value(3)), ("m_nCollisionAttributeIndex", int_value(1)), ("m_Compound", compound)]);
    let part = obj(vec![("m_rnShape", obj(vec![("m_compounds", list([desc]))]))]);
    let Value::Object(agg) = obj(vec![("m_parts", list([part]))]) else { unreachable!() };
    let s = &super::physics::from_kv3(&agg).parts[0].shapes;
    assert_eq!((s.spheres[0].2.surface, s.capsules[0].3.surface, s.capsules[0].3.attribute), (5, 7, 1));
}

#[test]
fn rigged_model_reads_rig_morphs_and_key_values() {
    let m = rigged();
    assert_eq!((m.attachments.clone(), m.hitbox_sets.clone()), (fixture::attachments(), fixture::hitbox_sets()));
    assert_eq!(model::bones(&model::write(&fixture::rigged_model(), super::vbib::Packing::Raw)).unwrap(), fixture::rig_bones());
    let names: Vec<&str> = m.flex_controllers().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["smile", "blink"]);
    let mesh = &m.meshes[0];
    let set = mesh.morph.as_ref().unwrap();
    assert_eq!(set.weights(&[0.5, 1.0]), [0.5, 0.5], "blink is dominated by smile");
    let smile = &mesh.targets[0];
    assert_eq!(smile.deltas.iter().map(|d| d.0).collect::<Vec<_>>(), [20, 21, 22, 23]);
    assert!(smile.deltas.iter().all(|d| (d.1[2] - SMILE_LIFT).abs() < 1e-2 && d.1[0] == 0.0), "{:?}", smile.deltas);
    let blink = &mesh.targets[1];
    assert_eq!(blink.deltas.len(), 1);
    assert!((blink.deltas[0].1[0] + 1.0).abs() < 1e-2 && (blink.deltas[0].2[2] - 0.5).abs() < 1e-2, "{:?}", blink.deltas);
    let files = fixture::rigged_files();
    let rig = model::read_rig(&files[0].1, &|p| files.iter().find(|(n, _)| n == p).map(|(_, b)| b.clone())).unwrap();
    assert!(rig.meshes[0].mesh.vertices.is_empty() && rig.meshes[0].targets.is_empty());
    assert_eq!((&rig.animations, &rig.attachments, rig.flex_controllers()), (&m.animations, &m.attachments, m.flex_controllers()));
    let kv = m.key_value_tree().unwrap();
    let kv = kv.as_object().unwrap();
    assert_eq!(kv.str("anim_graph_resource"), Some("animgraphs/rigged.vanmgrph"));
    assert_eq!(m.anim_graph().as_deref(), Some("animgraphs/rigged.vanmgrph"));
    assert!(kv.get("prop_data").and_then(|v| v.as_object()).is_some());
}
