use super::fixture::{self, RIGGED, SMILE_LIFT};
use super::model::{self, Model};

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
    assert!(m.animations[1].delta);
    assert_eq!(m.animations[2].frames[2][0].position, [2.0, 0.0, 0.0]);
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
    let kv = m.key_value_tree().unwrap();
    let kv = kv.as_object().unwrap();
    assert_eq!(kv.str("anim_graph_resource"), Some("animgraphs/rigged.vanmgrph"));
    assert!(kv.get("prop_data").and_then(|v| v.as_object()).is_some());
}
