//! The gizmo stand-ins draw what their geometry shaders emit: lines `LineThickness` pixels wide
//! and screen-space sprites their size in pixels. Skips without a GPU or an sbox-public checkout.

use super::{program, standin, tests::core_dir, vertex};
use crate::mesh::layout::Layout;
use crate::mesh::render_mesh::{DrawCall, RenderMesh, TRIANGLES};
use crate::mesh::vertex::Vertex;
use crate::render::draw::{self, Target, gizmo, model, shaded::Shaded};
use crate::render::lighting::Lighting;
use crate::render::{device, material, texture};
use glam::{Mat4, Vec3, Vec4};
use render::RasterState;
use render::pass::{DrawUniforms, Load};
use std::collections::HashMap;
use std::sync::Arc;

const SIZE: u32 = 32;

/// Pixels `vertices` of `prim` cover when drawn through `kind`'s stand-in with `values`.
fn covered(kind: standin::Kind, prim: i64, vertices: &[Vertex], values: &[(&str, f32)]) -> Option<usize> {
    let (core, gpu) = (core_dir()?, device::create()?);
    let shader = standin::shader(kind.path(), &shaders::IncludePaths::new([core])).unwrap().unwrap();
    let s = Arc::new(program::build(gpu, &shaders::Compiler::new().unwrap(), &shader, kind.path(), &shaders::Variant::default()).unwrap());
    let inputs = vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).expect("Sandbox.Vertex feeds the stand-in");
    let (corners, indices) = gizmo::expand(kind, prim, vertices).unwrap();
    let mesh = RenderMesh::from_vertices("gizmo test", &corners, &indices);
    let view_proj = render::Camera::perspective(60f32.to_radians(), 1.0, 1.0, Vec3::ZERO, Vec3::X).view_proj;
    let call = DrawCall { prim: TRIANGLES, index_count: indices.len() as i32, vertex_count: corners.len() as i32, ..Default::default() };
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let mut d = model::draw_call(&mesh, &call, uniforms, view_proj, RasterState::default()).unwrap();
    let mat = material::create_raw(Some(c"gizmo test"), Some(c"shaders/gizmo/gizmo_line.shader"), true);
    let values = values.iter().map(|(k, v)| (k.to_string(), Vec4::splat(*v))).collect::<HashMap<_, _>>();
    let instance = super::bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] };
    d.shaded = Some(Shaded { shading: s, vertex: inputs.keys, material: material::resolve(mat).unwrap(), values, instance, view_proj, context: Default::default() });
    let color = texture::create_target("gizmo", SIZE, SIZE, 0);
    draw::execute(&Target { color, ..Default::default() }, &Lighting::fallback(), Load { color: Some(render::wgpu::Color::BLACK), depth: None }, &[d]);
    let frame = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);
    crate::resource::destroy_handle(color);
    crate::resource::destroy_handle(mat);
    Some((0..SIZE).flat_map(|y| (0..SIZE).map(move |x| (x, y))).filter(|&(x, y)| frame.texel(x, y)[1] > 128).count())
}

fn at(y: f32, z: f32, tex0: [f32; 4]) -> Vertex {
    Vertex { position: [10.0, y, z], tex0, ..Vertex::default() }
}

#[test]
fn lines_are_as_wide_as_their_thickness() {
    let line = [at(-3.0, 0.0, [0.0; 4]), at(3.0, 0.0, [0.0; 4])];
    let Some(thin) = covered(standin::Kind::GizmoLine, 1, &line, &[("LineThickness", 1.0)]) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let thick = covered(standin::Kind::GizmoLine, 1, &line, &[("LineThickness", 6.0)]).unwrap();
    assert!(thin > 8 && (5 * thin..=7 * thin).contains(&thick), "1 px: {thin}, 6 px: {thick}");
}

#[test]
fn screen_space_sprites_cover_their_size_in_pixels() {
    let Some(n) = covered(standin::Kind::GizmoSprite, 0, &[at(0.0, 0.0, [8.0, 6.0, 0.0, 0.0])], &[]) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    assert!((40..=56).contains(&n), "an 8×6 sprite covered {n} px");
}
