use super::glue::{json_to_kv3, kv1_to_json, kv3_to_json, load_kv3};
use super::node::{Data, Node, get, handle, release};
use super::*;
use kv::Value;

fn root() -> (Arc<Node>, Ref) {
    let n = Node::new(Data::Null);
    let r = handle(&n);
    (n, r)
}

/// Model.Save's CTRL block: members, an array of tables, and Save().
#[test]
fn builds_ctrl_block_like_model_save() {
    let (ctrl, r) = root();
    let meshes = ctrl.find_or_create("embedded_meshes");
    meshes.set(Data::Array(Vec::new()));
    let mesh = meshes.push();
    mesh.set(Data::Table(Vec::new()));
    mesh.find_or_create("name").set(Data::String { text: "mesh#0".into(), resource: false });
    mesh.find_or_create("mesh_index").set(Data::Int(0));
    ctrl.find_or_create("embedded_physics").find_or_create("phys_data_block").set(Data::Int(2));

    let (_, parsed) = kv::kv3::parse(&save(r)).unwrap();
    let o = parsed.as_object().unwrap();
    let mesh = &o.get("embedded_meshes").unwrap().as_array().unwrap()[0];
    assert_eq!(mesh.as_object().unwrap().str("name"), Some("mesh#0"));
    assert_eq!(o.get("embedded_physics").unwrap().as_object().unwrap().get("phys_data_block"), Some(&Value::Int(2)));
    release(&ctrl);
}

#[test]
fn handles_are_stable_and_freed_with_the_tree() {
    let (n, r) = root();
    let child = n.find_or_create("a");
    let c1 = handle(&child);
    assert_eq!(handle(&n.find_or_create("a")), c1, "same member, same pointer");
    assert_eq!(handle(&n), r);
    n.set(Data::Null);
    assert!(get(c1).is_none(), "overwriting a table frees its members' handles");
    release(&n);
    assert!(get(r).is_none());
}

/// SceneModel.Events reads anim event payloads with defaults.
#[test]
fn member_reads_convert_and_default() {
    let r = load_kv3("{ Int = 3 Float = 2.5 Vector = [1, 2, 3.5] TypeName = \"x\" color = [10, 20, 300] s = \"7\" }");
    let n = get(r).unwrap();
    let m = |k: &str| n.member(k).unwrap();
    assert_eq!(m("Int").lock().as_i64(), Some(3));
    assert_eq!(m("Float").lock().as_i64(), Some(2));
    assert_eq!(m("Int").lock().as_f64(), Some(3.0));
    assert_eq!(m("s").lock().as_i64(), Some(7));
    assert_eq!(m("TypeName").lock().as_string(), "x");
    let v = m("Vector").lock().as_vector().unwrap();
    assert_eq!((v.vec.x, v.vec.y, v.vec.z), (1.0, 2.0, 3.5));
    let c = m("color").lock().as_color().unwrap();
    assert_eq!((c.r, c.g, c.b, c.a), (10, 20, 255, 255));
    assert!(n.member("missing").is_none());
    assert!(n.member("int").is_none(), "member names are case-sensitive");
    assert_eq!(m("TypeName").lock().as_i64(), None);
    release(&n);
}

#[test]
fn json_round_trip_and_kv1() {
    let r = json_to_kv3(r#"{"a":1,"b":[true,"s"],"c":{"d":0.5}}"#);
    let back: serde_json::Value = serde_json::from_str(&kv3_to_json(r).unwrap()).unwrap();
    assert_eq!(back, serde_json::json!({"a":1,"b":[true,"s"],"c":{"d":0.5}}));
    assert_eq!(get(r).unwrap().lock().type_id(), 9);
    release(&get(r).unwrap());
    assert!(json_to_kv3("{nope").is_null());
    assert!(load_kv3("{ a = ").is_null());

    let json = kv1_to_json("\"config\" { \"convars\" { \"sensitivity\" \"2\" } }").unwrap();
    assert_eq!(json, r#"{"convars":{"sensitivity":"2"}}"#);
}

#[test]
fn resource_strings_save_flagged() {
    let (n, r) = root();
    n.find_or_create("model").set(Data::String { text: "a.vmdl".into(), resource: true });
    assert!(save(r).contains("model = resource:\"a.vmdl\""));
    release(&n);
}
