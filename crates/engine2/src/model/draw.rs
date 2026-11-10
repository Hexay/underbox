//! `CModel`'s draw calls (one scene object per mesh) and `MeshTrace` against its triangles.

use super::{Model, on};
use crate::mesh::geometry::ray_triangle;
use crate::mesh::material_ref;
use crate::mesh::render_mesh::{DrawCall, TRIANGLES};
use abi::slots::c_model as m;
use crate::math::{from_v3 as out, v3 as v};
use abi::structs::{MeshTraceInput, MeshTraceOutput, System_Numerics_Vector2, Vector2};
use abi::{Registry, bind};
use glam::Vec3;

fn call<R: Default>(r: super::Ref, mesh: i32, dc: i32, f: impl FnOnce(&DrawCall) -> R) -> R {
    on(r, |m| m.content.mesh(mesh).and_then(|x| x.mesh.draw_calls().get(usize::try_from(dc).ok()?).map(f)).unwrap_or_default())
}

/// The nearest triangle the segment `start`–`end` crosses, over every LOD-0 triangle mesh.
pub fn trace(m: &Model, input: &MeshTraceInput, hit: &mut MeshTraceOutput) -> bool {
    let (start, end) = (v(input.start), v(input.end));
    let (dir, length) = ((end - start).normalize_or_zero(), (end - start).length());
    if dir == Vec3::ZERO {
        return false;
    }
    let mut best: Option<(f32, Vec3, i32, Vector2)> = None;
    let mut triangle_base = 0;
    for x in m.content.meshes.iter().filter(|x| x.lod_mask & 1 != 0) {
        let (vertices, indices) = (x.mesh.vertices(), x.mesh.indices());
        for d in x.mesh.draw_calls().iter().filter(|d| d.prim == TRIANGLES) {
            let start_index = d.start_index.max(0) as usize;
            let span = indices.get(start_index..start_index + d.index_count.max(0) as usize).unwrap_or_default();
            for (t, tri) in span.as_chunks::<3>().0.iter().enumerate() {
                let corner = |i: u32| vertices.get(i as usize + d.start_vertex.max(0) as usize).copied().unwrap_or_default();
                let c = [corner(tri[0]), corner(tri[1]), corner(tri[2])];
                let p = c.map(|c| Vec3::from(c.position));
                let Some(h) = ray_triangle(start, dir, p) else { continue };
                if h.distance > length || best.is_some_and(|b| b.0 <= h.distance) {
                    continue;
                }
                let w = 1.0 - h.u - h.v;
                let uv = |k: usize| glam::Vec2::new(c[k].tex0[0], c[k].tex0[1]);
                let uv = uv(0) * w + uv(1) * h.u + uv(2) * h.v;
                best = Some((h.distance, (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero(), triangle_base + (start_index / 3 + t) as i32, Vector2 { vec: System_Numerics_Vector2 { x: uv.x, y: uv.y } }));
            }
        }
        triangle_base += (indices.len() / 3) as i32;
    }
    let Some((distance, normal, triangle, uv)) = best else { return false };
    let normal = if normal.dot(dir) > 0.0 { -normal } else { normal };
    (hit.distance, hit.position, hit.normal, hit.triangle_index, hit.uv) = (distance, out(start + dir * distance), out(normal), triangle, uv);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, m::GET_NUM_SCENE_OBJECTS, |r, mesh| on(r, |m| i32::from(m.content.mesh(mesh).is_some())));
    bind!(r, m::GET_NUM_DRAW_CALLS, |r, mesh, so| on(r, |m| if so == 0 { m.content.mesh(mesh).map_or(0, |x| x.mesh.draw_calls().len() as i32) } else { 0 }));
    bind!(r, m::GET_MATERIAL, |r, mesh, so, dc| if so == 0 { call(r, mesh, dc, |d| material_ref::handle(d.material.as_ref())) } else { Default::default() });
    bind!(r, m::GET_DRAW_CALL_MATERIAL, |r, mesh, dc| call(r, mesh, dc, |d| material_ref::handle(d.material.as_ref())));
    bind!(r, m::GET_DRAW_CALL_INFO, |r, mesh, dc, vertices, indices, prim, uv_density, instances, alpha, start, base| {
        let found = on(r, |m| {
            let x = m.content.mesh(mesh)?;
            Some((x.mesh.draw_calls().get(usize::try_from(dc).ok()?)?.clone(), x.mesh.state().uv_density))
        });
        let instances_out = i32::from(found.is_some());
        let (d, density) = found.unwrap_or_default();
        (*vertices, *indices, *prim, *uv_density, *instances, *alpha, *start, *base) = (d.vertex_count, d.index_count, d.prim, density, instances_out, 0, d.start_index, d.start_vertex);
    });
    bind!(r, m::MESH_TRACE, |r, input, output| on(r, |m| trace(m, &input, output)));
}
