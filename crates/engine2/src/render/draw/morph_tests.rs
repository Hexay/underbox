use super::*;
use crate::mesh::render_mesh::MorphDelta;
use crate::mesh::vertex::Vertex;
use assets::compiled::flex::{Controller, Op, Rule};

fn vertex(position: [f32; 3]) -> Vertex {
    Vertex { position, color: [255; 4], normal: [0.0, 0.0, 1.0], tex0: [0.0; 4], tex1: [0.0; 4], tangent: [1.0, 0.0, 0.0, 1.0] }
}

/// A triangle whose "smile" target lifts vertex 1 by 3 units, under a set where descriptor
/// "smile" is twice controller "jaw".
fn fixture() -> (Arc<RenderMesh>, MorphSet) {
    let mesh = RenderMesh::from_vertices("morphed", &[vertex([0.0; 3]), vertex([1.0, 0.0, 0.0]), vertex([0.0, 1.0, 0.0])], &[0, 1, 2]);
    mesh.state().morphs = vec![Morph { name: "smile".into(), deltas: vec![MorphDelta { vertex: 1, position: [0.0, 0.0, 3.0], normal: [0.0; 3] }] }];
    let set = MorphSet {
        descriptors: vec!["smile".into()],
        controllers: vec![Controller { name: "jaw".into(), kind: "default".into(), min: 0.0, max: 1.0 }],
        rules: vec![Rule { flex: 0, ops: vec![Op::Fetch1(0), Op::Const(2.0), Op::Mul] }],
        ..Default::default()
    };
    (mesh, set)
}

fn position(vb: &VertexBuffer, v: usize) -> [f32; 3] {
    let at = v * vb.stride();
    let bytes = vb.read();
    std::array::from_fn(|i| f32::from_le_bytes(bytes[at + 4 * i..at + 4 * i + 4].try_into().unwrap()))
}

#[test]
fn a_flex_weight_moves_the_morphed_vertex() {
    let (mesh, set) = fixture();
    let w = weights(Some(&set), &["jaw".into()], &[0.25]);
    assert_eq!(w.get("smile"), Some(&0.5));
    let morphs = ObjectMorphs { object: Weak::new(), key: 1, meshes: vec![w] };
    let morphed = vertices(&morphs, 0, &mesh).expect("a weighted target morphs");
    assert_eq!(position(&morphed, 1), [1.0, 0.0, 1.5]);
    assert_eq!(position(&morphed, 0), [0.0; 3], "vertices without deltas stay");
    let base = mesh.state().vertices.clone().unwrap();
    assert_eq!(position(&base, 1), [1.0, 0.0, 0.0], "the mesh's own buffer is untouched");
    assert!(Arc::ptr_eq(&morphed, &vertices(&morphs, 0, &mesh).unwrap()), "same weights reuse the copy");
}

#[test]
fn zero_weight_and_unnamed_targets_leave_the_mesh() {
    let (mesh, set) = fixture();
    for w in [weights(Some(&set), &["jaw".into()], &[0.0]), weights(None, &["brow".into()], &[1.0])] {
        let morphs = ObjectMorphs { object: Weak::new(), key: 2, meshes: vec![w] };
        assert!(vertices(&morphs, 0, &mesh).is_none());
    }
    // Without a set, a controller weighs the target of its own name.
    let morphs = ObjectMorphs { object: Weak::new(), key: 3, meshes: vec![weights(None, &["smile".into()], &[1.0])] };
    assert_eq!(position(&vertices(&morphs, 0, &mesh).unwrap(), 1), [1.0, 0.0, 3.0]);
}
