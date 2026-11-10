//! `MeshGlue`'s geometry utilities (MeshUtils.cs, Mesh.cs): polygon triangulation, clipping a
//! segment to a polygon, and UVs for triangles.

use super::geometry::{self, plane_basis};
use crate::containers;
use crate::math::{from_v3 as out, v3 as v, vec3};
use abi::slots::mesh_glue as g;
use abi::structs::Vector3;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use glam::{Vec2, Vec3};
use std::ffi::c_void;

fn points(ptr: RawPtr<c_void>, n: i32) -> Vec<Vec3> {
    // SAFETY: managed passes a pinned Vector3[n].
    unsafe { ptr.cast::<Vector3>().as_slice(n.max(0) as usize) }.iter().copied().map(v).collect()
}

/// Write up to `max` indices; the count written.
fn triangulate(verts: RawPtr<c_void>, n: i32, indices: RawPtr<c_void>, max: i32) -> i32 {
    let flat: Vec<i32> = geometry::triangulate(&points(verts, n)).into_iter().flatten().map(|i| i as i32).collect();
    let count = flat.len().min(max.max(0) as usize);
    // SAFETY: managed passes a pinned int[max] (MeshUtils.TriangulatePolygon).
    unsafe { indices.cast::<i32>().as_mut_slice(count) }.copy_from_slice(&flat[..count]);
    count as i32
}

fn clip(verts: RawPtr<c_void>, n: i32, a: Vector3, b: Vector3, result: NativeRef<class::CUtlVector_Vector_>) {
    let pts = geometry::clip_segment(&points(verts, n), v(a), v(b));
    containers::with(result, |o: &mut Vec<Vector3>| o.extend(pts.into_iter().map(out)));
}

/// `AlignEdgeUV_t`.
const ALIGN_U: i64 = 1;
const ALIGN_V: i64 = 2;

/// Per-vertex UVs in 0..1. Each face (or each triangle without face ids) is projected onto its
/// own plane; the whole set is then scaled uniformly into the unit square.
// TODO: real LSCM unwrapping (`GenerateUVsMode_t`); this is a planar projection per face.
pub fn generate_uvs(positions: &[Vec3], tris: &[[u32; 3]], faces: Option<&[i32]>, align: i64, edge: (i32, i32)) -> Vec<Vec2> {
    let mut uv = vec![Vec2::ZERO; positions.len()];
    let at = |i: u32| positions.get(i as usize).copied().unwrap_or_default();
    let face_of = |t: usize| faces.and_then(|f| f.get(t).copied()).unwrap_or(t as i32);
    let mut ids: Vec<i32> = (0..tris.len()).map(face_of).collect();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let members: Vec<&[u32; 3]> = tris.iter().enumerate().filter(|(t, _)| face_of(*t) == id).map(|(_, t)| t).collect();
        let n = members.iter().fold(Vec3::ZERO, |n, t| n + (at(t[1]) - at(t[0])).cross(at(t[2]) - at(t[0]))).normalize_or_zero();
        let (mut u, mut w) = plane_basis(if n == Vec3::ZERO { Vec3::Z } else { n });
        if let (Ok(a), Ok(b)) = (u32::try_from(edge.0), u32::try_from(edge.1))
            && (align == ALIGN_U || align == ALIGN_V)
        {
            let d = (at(b) - at(a)).reject_from(n).normalize_or_zero();
            if d != Vec3::ZERO {
                (u, w) = if align == ALIGN_U { (d, n.cross(d)) } else { (d.cross(n), d) };
            }
        }
        for i in members.iter().flat_map(|t| t.iter()) {
            if let Some(slot) = uv.get_mut(*i as usize) {
                *slot = Vec2::new(at(*i).dot(u), at(*i).dot(w));
            }
        }
    }
    let lo = uv.iter().fold(Vec2::splat(f32::MAX), |a, b| a.min(*b));
    let hi = uv.iter().fold(Vec2::splat(f32::MIN), |a, b| a.max(*b));
    let size = (hi - lo).max_element();
    if size > 0.0 { uv.iter().map(|p| (*p - lo) / size).collect() } else { uv }
}

#[allow(clippy::too_many_arguments)]
fn uvs(n: i32, positions: RawPtr<c_void>, n_tris: i32, indices: RawPtr<c_void>, faces: RawPtr<c_void>, _mode: i64, align: i64, a: i32, b: i32, result: NativeRef<class::CUtlVector_Vector_>) {
    let positions = points(positions, n);
    // SAFETY: managed passes pinned uint[n_tris * 3] and, unless null, int[n_tris] (MeshUtils.cs).
    let (tris, faces) = unsafe { (indices.cast::<[u32; 3]>().as_slice(n_tris.max(0) as usize), (!faces.is_null()).then(|| faces.cast::<i32>().as_slice(n_tris.max(0) as usize))) };
    let uv = generate_uvs(&positions, tris, faces, align, (a, b));
    containers::with(result, |o: &mut Vec<Vector3>| o.extend(uv.into_iter().map(|p| vec3(p.x, p.y, 0.0))));
}

pub fn register(r: &mut Registry) {
    bind!(r, g::TRIANGULATE_POLYGON, triangulate);
    bind!(r, g::CLIP_POLYGON_LINE_SEGMENT, clip);
    bind!(r, g::GENERATE_U_VS_FOR_TRIANGLES, uvs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_uvs_fill_unit_square_and_align_to_edge() {
        let p = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 0.0), Vec3::new(0.0, 4.0, 0.0)];
        let tris = [[0, 1, 2], [0, 2, 3]];
        let uv = generate_uvs(&p, &tris, Some(&[7, 7]), ALIGN_U, (0, 1));
        assert_eq!(uv.len(), 4);
        assert!(uv.iter().all(|u| (-1e-5..=1.0 + 1e-5).contains(&u.x) && (-1e-5..=1.0 + 1e-5).contains(&u.y)), "{uv:?}");
        assert!((uv[1].y - uv[0].y).abs() < 1e-5 && (uv[1].x - uv[0].x - 1.0).abs() < 1e-5, "edge 0-1 runs along U: {uv:?}");
        assert_eq!(geometry::newell_normal(&p), Vec3::Z);
    }
}
