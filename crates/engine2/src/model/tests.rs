//! Runtime models: body parts and LODs, the builder's content (key values, physics), mesh traces,
//! and physics data through its desc array and part/joint views.

use super::content::{ALL_LODS, Content, ModelMesh};
use super::physics::data::PhysicsData;
use super::physics::{data_slots, desc, views};
use super::{builder, draw, parts};
use crate::kv3;
use crate::math::{identity, vec3};
use crate::mesh::render_mesh::RenderMesh;
use crate::mesh::vertex::Vertex;
use abi::structs::{MeshTraceInput, MeshTraceOutput, Transform};
use abi::{NativeRef, class};
use kv::{Object, Value};
use std::sync::Arc;

fn quad(z: f32) -> Arc<RenderMesh> {
    let at = |x, y| Vertex { position: [x, y, z], tex0: [x, y, 0.0, 0.0], ..Default::default() };
    RenderMesh::from_vertices("quad", &[at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)], &[0, 1, 2, 0, 2, 3])
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform { position: vec3(x, y, z), ..identity() }
}

/// Two parts, `pelvis` (z = 50) and `spine` (z = 70), 10-unit boxes joined by a ball joint
/// half way between them, with a ±45° swing limit.
pub(crate) fn ragdoll() -> Arc<PhysicsData> {
    let a = desc::create(2, 1);
    let d = objects::get::<_, desc::DescArray>(a).unwrap();
    for (b, (bone, z)) in d.bodies.iter().zip([("pelvis", 50.0), ("spine", 70.0)]) {
        desc::with_body(b.as_raw(), |p| {
            p.bone = bone.into();
            p.bind = at(0.0, 0.0, z);
            p.mass = 10.0;
        });
        desc::add_box(b.as_raw(), vec3(5.0, 5.0, 5.0), identity());
    }
    let j = desc::joint(d.joints[0]).unwrap();
    {
        let mut j = j.lock().unwrap();
        (j.kind, j.body1, j.body2) = (1, 0, 1);
        (j.frame1, j.frame2) = (at(0.0, 0.0, 10.0), at(0.0, 0.0, -10.0));
        (j.enable_swing_limit, j.swing_limit) = (true, [-0.785, 0.785]);
    }
    let data = desc::build("ragdoll", a).unwrap();
    desc::delete(a);
    data
}

fn builder_ref(tag: usize) -> builder::Ref {
    NativeRef::from_raw((0x7e57_0000 + tag) as *mut _)
}

#[test]
fn body_parts_come_from_mesh_group_names() {
    let content = Content {
        meshes: vec![ModelMesh { mesh: quad(0.0), group_mask: 1, lod_mask: ALL_LODS }, ModelMesh { mesh: quad(0.0), group_mask: 4, lod_mask: 0b10 }],
        groups: vec!["body_@0".into(), "body_@1".into(), "head".into()],
        lod_distances: vec![0.0, 50.0],
        ..Default::default()
    };
    let m = super::get(super::create_runtime("models/test/parts.vmdl", content)).unwrap();
    let parts = parts::parts(&m);
    let summary: Vec<_> = parts.iter().map(|p| (p.name.as_str(), p.groups.clone())).collect();
    assert_eq!(summary, vec![("body", vec![0, 1]), ("head", vec![2])]);
    assert_eq!(parts::group_name(&m, 0).as_deref(), Some("default"), "a model without skins has one");
    assert_eq!(parts::max_lod(&m), 1);
    assert_eq!(parts::lod_for_screen_size(&m, 10.0, 1.0), 1);
    assert_eq!(parts::lod_for_screen_size(&m, 100.0, 1.0), 0);
}

#[test]
fn builder_content_reaches_the_model() {
    let b = builder_ref(1);
    let mesh = crate::resource::new_handle(quad(0.0));
    builder::add_mesh(b, mesh, 1, -1);
    assert_eq!(builder::add_mesh_group(b, Some(c"main")), 0);
    builder::set_lod_distance(b, 1, 30.0);
    builder::set_lod_distance(b, 9, 1.0);

    let tree = Value::Object(Object(vec![("breakable".into(), Value::Object(Object(vec![("health".into(), Value::Int(25))])))]));
    let kv = kv3::create(&tree);
    assert!(builder::set_key_values(b, kv));
    // Managed deletes its tree right after; the model keeps a copy.
    kv3::node::release(&kv3::node::get(kv).unwrap());

    builder::set_physics(b, {
        let a = desc::create(1, 0);
        desc::add_box(objects::get::<_, desc::DescArray>(a).unwrap().bodies[0].as_raw(), vec3(1.0, 1.0, 1.0), identity());
        a
    });
    let m = super::get(super::create_runtime("models/test/built.vmdl", builder::take(b))).unwrap();
    assert!(builder::take(b).meshes.is_empty(), "taken once");

    assert_eq!((m.content.meshes.len(), m.content.groups.len()), (1, 1));
    assert_eq!(m.content.meshes[0].lod_mask, ALL_LODS);
    assert_eq!(m.content.lod_distances, vec![0.0, 30.0]);
    let health = m.content.game_data.as_ref().and_then(|d| d.member("breakable")).and_then(|n| n.member("health"));
    assert_eq!(health.map(|n| kv3::to_value(&n)), Some(Value::Int(25)));
    let physics = m.content.physics.as_ref().unwrap();
    assert_eq!(physics.parts[0].shapes.hulls[0].hull.vertices.len(), 8);
}

#[test]
fn trace_hits_the_nearest_triangle() {
    let content = Content { meshes: vec![ModelMesh { mesh: quad(0.0), group_mask: 1, lod_mask: ALL_LODS }, ModelMesh { mesh: quad(5.0), group_mask: 1, lod_mask: ALL_LODS }], ..Default::default() };
    let m = super::get(super::create_runtime("models/test/trace.vmdl", content)).unwrap();
    let ray = |x, y| {
        let mut input = MeshTraceInput::default();
        (input.start, input.end) = (vec3(x, y, 10.0), vec3(x, y, -10.0));
        input
    };
    let mut hit = MeshTraceOutput::default();
    assert!(draw::trace(&m, &ray(0.75, 0.25), &mut hit));
    assert!((hit.distance - 5.0).abs() < 1e-4, "{}", hit.distance);
    assert_eq!((hit.normal.vec.z, hit.triangle_index), (1.0, 2));
    assert!((hit.uv.vec.x - 0.75).abs() < 1e-4 && (hit.uv.vec.y - 0.25).abs() < 1e-4);
    assert!(!draw::trace(&m, &ray(3.0, 3.0), &mut hit));
}

#[test]
fn physics_data_reads_through_part_and_joint_views() {
    let d = ragdoll();
    assert_eq!((d.parts.len(), d.joints.len()), (2, 1));
    assert!(d.has_bones());
    let hull = &d.parts[1].shapes.hulls[0].hull;
    assert!((hull.volume() - 1000.0).abs() < 1e-2);

    let p: NativeRef<class::VPhysXBodyPart_t> = d.part_ref(1);
    assert_eq!(views::part(p.as_raw(), |v| v.part.bone.clone()), "spine");
    let j = objects::get::<_, std::sync::Mutex<super::physics::data::JointDesc>>(d.joint_ref(0)).unwrap();
    assert_eq!(j.lock().unwrap().body2, 1);

    let b = data_slots::bounds(&d);
    assert_eq!((b.mins.vec.z, b.maxs.vec.z), (45.0, 75.0));
}
