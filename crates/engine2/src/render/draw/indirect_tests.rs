//! Indirect draws read their arguments on the GPU: the context's bound-buffer forms through the
//! placeholder, and `DrawModel_1/4` through managed's stand-in program as draw_call_tests.rs sets
//! it up (skipped without an sbox-public checkout).

use super::super::draw_call::tests::{gpu_buffer, managed_view_constants, placed_entry};
use super::super::immediate::{self, Indices, Vertices};
use super::{draw_bound, instanced, transformed};
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::RenderMesh;
use crate::mesh::vertex::Vertex;
use crate::render::{attributes, buffers, context, device, material, texture};
use abi::StringToken;
use glam::{Mat4, Vec3};

const SIZE: u32 = 64;
const CLEAR: [u8; 4] = [0, 0, 255, 255];

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The middle pixel of `color` after clearing it and running `draw`.
fn middle(c: context::Ref, color: texture::Ref, draw: impl FnOnce()) -> [u8; 4] {
    context::clear(c, crate::math::vec4(0.0, 0.0, 1.0, 1.0), true, true, true);
    draw();
    let px = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(device::gpu().unwrap());
    px.texel(SIZE / 2, SIZE / 2)[..4].try_into().unwrap()
}

/// A triangle covering clip space, as the context's identity view sees it.
fn screen_triangle() -> std::sync::Arc<RenderMesh> {
    let v = |x: f32, y: f32| Vertex { position: [x, y, 0.5], color: [255; 4], normal: [0.0, 0.0, 1.0], tex0: [0.0; 4], tex1: [0.0; 4], tangent: [1.0, 0.0, 0.0, 1.0] };
    RenderMesh::from_vertices("screen", &[v(-1.0, -1.0), v(3.0, -1.0), v(-1.0, 3.0)], &[0, 1, 2])
}

#[test]
fn context_draws_take_counts_from_the_args_buffer() {
    device::create().expect("a wgpu adapter");
    let c = context::create(0);
    let color = texture::create_target("indirect", SIZE, SIZE, 0);
    context::bind_targets(c, color, Default::default());
    let mesh = screen_triangle();
    let (vb, ib) = {
        let state = mesh.state();
        (state.vertices.clone().unwrap(), state.indices.clone().unwrap())
    };
    immediate::bind_vertices(c, 0, Some(Vertices::Mesh(vb)), 0);
    let drawn = |args: &[u32], offset, count, stride, indexed| {
        let b = gpu_buffer(&words(args), 4);
        let px = middle(c, color, || draw_bound(c, 5, b, offset, count, stride, indexed));
        buffers::destroy(b);
        px != CLEAR
    };
    let plain = [3, 0, 0, 0, 3, 1, 0, 0];
    assert!(!drawn(&plain, 0, 1, 0, false), "no instances");
    assert!(drawn(&plain, 16, 1, 0, false), "one instance");
    assert!(drawn(&plain, 0, 2, 0, false), "the second of two packed draws");
    assert!(!drawn(&plain, 0, 0, 0, false), "no draws");
    assert!(!drawn(&plain, 2, 1, 0, false), "a misaligned offset draws nothing");
    assert!(!drawn(&plain, 16, 1, 0, true), "indexed, without an index buffer");
    immediate::bind_indices(c, Some(Indices::Mesh(ib)));
    let indexed = [3, 0, 0, 0, 0, 0, 3, 1, 0, 0, 0, 0];
    assert!(!drawn(&indexed, 0, 1, 0, true), "no instances");
    assert!(drawn(&indexed, 24, 1, 0, true), "one instance");
    assert!(drawn(&indexed, 0, 2, 24, true), "strided");
    assert!(!drawn(&indexed, 40, 1, 0, true), "args past the buffer's end draw nothing");
    crate::resource::destroy_handle(color);
    context::release(c);
}

#[test]
fn draw_model_reads_instances_and_transforms_from_buffers() {
    let Some(core) = crate::render::shading::tests::core_dir() else {
        eprintln!("skipped: no sbox-public checkout");
        return;
    };
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    device::create().expect("a wgpu adapter");
    let (segment, draw) = (attributes::create(), attributes::create());
    let c = context::create(0);
    attributes::set_parent(context::get(c).unwrap().attributes, segment);
    let constants = managed_view_constants(Vec3::new(-30.0, 0.0, 30.0), Vec3::ZERO, 90.0, (1.0, 1000.0));
    attributes::set_constant_buffer(segment, StringToken::of(crate::render::shading::view::BLOCK), &constants);
    let (color, depth) = (texture::create_target("model", SIZE, SIZE, 0), texture::create_target("model depth", SIZE, SIZE, 62));
    context::bind_targets(c, color, depth);
    let model = crate::model::create_runtime("models/test/indirect.vmdl", Default::default());
    let mat = material::create_raw(Some(c"materials/test/indirect.vmat"), Some(c"shaders/complex.shader"), true);
    crate::model::get(model).unwrap().content.meshes[0].mesh.state().material = MaterialRef::keep(mat);

    // Two args records, 20 bytes apart; the offset is in bytes (Graphics.DrawModelInstancedIndirect).
    let counts = gpu_buffer(&words(&[0, 0, 0, 0, 0, 0, 1, 0]), 4);
    let empty = middle(c, color, || instanced(c, model, 0, (counts, 0), draw));
    assert_eq!(empty, CLEAR, "no instances, no box");
    let one = middle(c, color, || instanced(c, model, 0, (counts, 20), draw));
    assert_ne!(one, CLEAR, "the box from the second record's instance count, at byte 24");

    // Entry 0 far off screen, entry 1 at the origin in red; each args record draws one instance.
    let far = Mat4::from_translation(Vec3::new(0.0, 5000.0, 0.0));
    let transforms = gpu_buffer(&[placed_entry(far, 0xFF0000), placed_entry(Mat4::IDENTITY, 0xFF0000)].concat(), 64);
    let args = gpu_buffer(&words(&[36, 1, 0, 0, 0, 36, 1, 0, 0, 1]), 20);
    let red = |[r, g, b, _]: [u8; 4]| r > 60 && g < 10 && b < 10;
    assert_eq!(middle(c, color, || transformed(c, model, 0, transforms, (args, 0), draw)), CLEAR, "instance 0 is placed off screen");
    let placed = middle(c, color, || transformed(c, model, 0, transforms, (args, 20), draw));
    assert!(red(placed), "instance 1 is the red box in the middle, got {placed:?}");
    for b in [counts, transforms, args] {
        buffers::destroy(b);
    }
    for t in [color, depth] {
        crate::resource::destroy_handle(t);
    }
    crate::resource::destroy_handle(model);
    crate::resource::destroy_handle(mat);
    context::release(c);
}
