//! `MaterialSystem2Utils.DrawScreenSpaceRectangle`: a material drawn over a pixel rectangle of
//! the context's target, its texture coordinates spanning `uv0`–`uv1` (in texels of a `tex`
//! sized texture when given), without depth.

use super::model as model_draw;
use super::shaded::{self, ContextBindings};
use super::immediate;
use crate::mesh::render_mesh::{DrawCall, RenderMesh, TRIANGLES};
use crate::mesh::vertex::Vertex;
use crate::render::shading::bind::Instance;
use crate::render::{attributes, context, material, texture};
use glam::{Mat4, Vec2};
use render::RasterState;
use render::pass::DrawUniforms;
use render::wgpu::CompareFunction;

#[allow(clippy::too_many_arguments)]
pub fn draw(c: context::Ref, mat: material::Ref, attrs: attributes::Ref, (x, y, w, h): (i32, i32, i32, i32), (u0, v0, u1, v1): (f32, f32, f32, f32), (tw, th): (i32, i32)) {
    let Some(material) = material::resolve(mat) else { return };
    let t = context::target(c);
    let texture = || texture::resolve(t.color).and_then(|x| x.gpu.as_ref().map(|g| Vec2::new(g.desc.width as f32, g.desc.height as f32)));
    let viewport = (t.viewport.m_n_width > 0 && t.viewport.m_n_height > 0).then(|| Vec2::new(t.viewport.m_n_width as f32, t.viewport.m_n_height as f32));
    let size = viewport.or_else(texture).unwrap_or(Vec2::ONE);
    // Pixels to clip space, y down; depth is unused.
    let projection = Mat4::from_scale(glam::Vec3::new(2.0 / size.x, -2.0 / size.y, 1.0)) * Mat4::from_translation(glam::Vec3::new(-size.x / 2.0, -size.y / 2.0, 0.0));
    let texel = Vec2::new(if tw > 0 { 1.0 / tw as f32 } else { 1.0 }, if th > 0 { 1.0 / th as f32 } else { 1.0 });
    let corner = |px: f32, py: f32, u: f32, v: f32| Vertex { position: [px, py, 0.0], color: [255; 4], normal: [0.0, 0.0, 1.0], tex0: [u * texel.x, v * texel.y, 0.0, 0.0], tex1: [0.0; 4], tangent: [1.0, 0.0, 0.0, 1.0] };
    let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
    let quad = [corner(x0, y0, u0, v0), corner(x1, y0, u1, v0), corner(x1, y1, u1, v1), corner(x0, y1, u0, v1)];
    let mesh = RenderMesh::from_vertices("screen rectangle", &quad, &[0, 1, 2, 0, 2, 3]);
    let call = DrawCall { material: None, prim: TRIANGLES, start_index: 0, index_count: 6, start_vertex: 0, vertex_count: 4, tint: None };
    let uniforms = DrawUniforms { mvp: projection, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let raster = RasterState { depth_write: false, depth_compare: CompareFunction::Always, ..Default::default() };
    let Some(mut d) = model_draw::draw_call(&mesh, &call, uniforms, projection, raster) else { return };
    let Some(layout) = mesh.state().vertices.as_ref().map(|v| v.meta.clone()) else { return };
    let bindings = ContextBindings { attributes: vec![attrs, context::get(c).map(|x| x.attributes).unwrap_or_default()], slots: immediate::with(c, |s| s.slots.clone()), instance_ids: None };
    d.shaded = shaded::resolve(material, &layout, Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }, projection, bindings);
    immediate::submit(c, &[d]);
}
