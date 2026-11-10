//! Runtime meshes: buffers as managed fills them, draw calls read back, and serialised blocks.

use super::buffers::{self, create_indices, create_vertices, index_buffer, vertex_buffer};
use super::layout::Layout;
use super::model_reads::read_draw_call;
use super::render_mesh::{DrawCall, RenderMesh, TRIANGLES};
use super::serialize;
use super::vertex::{Vertex, stream};
use abi::RawPtr;
use std::ffi::c_void;

fn at(x: f32, y: f32, z: f32) -> Vertex {
    Vertex { position: [x, y, z], ..Default::default() }
}

fn ptr<T>(v: &[T]) -> RawPtr<c_void> {
    RawPtr::from_raw(v.as_ptr() as *mut c_void)
}

/// A unit quad in z = 0 as two triangles.
fn quad() -> std::sync::Arc<RenderMesh> {
    let v = [at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), at(1.0, 1.0, 0.0), at(0.0, 1.0, 0.0)];
    RenderMesh::from_vertices("quad", &v, &[0, 1, 2, 0, 2, 3])
}

#[test]
fn vertex_buffer_fills_locks_and_resizes_in_place() {
    let given = [at(1.0, 2.0, 3.0), at(4.0, 5.0, 6.0)];
    let h = create_vertices(3, Layout::sbox_vertex(), ptr(&given), 2);
    let b = vertex_buffer(h).unwrap();
    assert_eq!((b.count(), b.stride()), (3, 76));
    let decoded = b.meta.decode_all(&b.read());
    assert_eq!(decoded[1].position, [4.0, 5.0, 6.0]);
    assert_eq!(decoded[2].position, [0.0; 3]);

    let third = [at(7.0, 8.0, 9.0)];
    let before = b.version();
    buffers::set_data(vertex_buffer(h), ptr(&third), 76, 152);
    assert!(b.version() > before);
    assert_eq!(b.meta.decode_all(&b.read())[2].position, [7.0, 8.0, 9.0]);

    let locked = buffers::lock(vertex_buffer(h), 76, 76);
    // SAFETY: the lock covers one vertex at offset 76.
    unsafe { locked.cast::<Vertex>().as_mut_slice(1)[0].position = [0.5; 3] };
    assert_eq!(b.meta.decode_all(&b.read())[1].position, [0.5; 3]);
    assert!(buffers::lock(vertex_buffer(h), 76, 228).is_null(), "past the end");

    buffers::resize(&b, 5, b.stride());
    assert_eq!(vertex_buffer(h).unwrap().count(), 5, "same handle after resize");
    assert_eq!(b.meta.decode_all(&b.read())[2].position, [7.0, 8.0, 9.0]);
}

#[test]
fn narrow_and_wide_index_buffers_read_back() {
    let narrow = create_indices(3, false, ptr(&[0u16, 1, 2]), 3);
    assert_eq!(index_buffer(narrow).unwrap().indices(), vec![0, 1, 2]);
    let wide = create_indices(4, true, ptr(&[70000u32, 1]), 2);
    assert_eq!(index_buffer(wide).unwrap().indices(), vec![70000, 1, 0, 0]);
}

#[test]
fn implied_draw_call_covers_the_mesh() {
    let m = quad();
    let calls = m.draw_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].prim, calls[0].index_count, calls[0].vertex_count), (TRIANGLES, 6, 4));
    let b = m.bounds();
    assert_eq!((b.mins.vec.x, b.maxs.vec.y, b.maxs.vec.z), (0.0, 1.0, 0.0));
    assert!(m.streams() & stream::POSITION != 0);
}

#[test]
fn read_draw_call_compacts_vertices() {
    let m = quad();
    let second = DrawCall { prim: TRIANGLES, start_index: 3, index_count: 3, vertex_count: 4, ..Default::default() };
    let (vertices, indices) = read_draw_call(&m, &second);
    assert_eq!(indices, vec![0, 1, 2]);
    let positions: Vec<_> = vertices.iter().map(|v| v.position).collect();
    assert_eq!(positions, vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
}

#[test]
fn serialised_mesh_blocks_parse_as_kv3() {
    let m = quad();
    let parse = |v: &kv::Value| kv::kv3::parse(&kv::write::kv3(v)).unwrap().1.as_object().cloned().unwrap();
    let buffers = parse(&serialize::mesh_buffers(&m));
    let vb = buffers.get("vertex_buffers").unwrap().as_array().unwrap()[0].as_object().unwrap();
    assert_eq!(vb.get("element_count").and_then(|v| v.as_f64()), Some(4.0));
    assert_eq!(vb.get("element_size").and_then(|v| v.as_f64()), Some(76.0));

    let data = parse(&serialize::mesh_data(&m));
    assert_eq!(data.str("name"), Some("quad"));
    assert_eq!(data.get("draw_calls").and_then(|d| d.as_array()).map(<[_]>::len), Some(1));
}
