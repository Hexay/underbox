//! `IPhysicsShape.GetTriangulation*`/`GetOutline`: a shape's collider as triangles or edges.
//! Triangulation and outline are in body space; the navmesh variant is in world space, keeping
//! only triangles that touch the bounds (HeightFieldGenerator feeds them straight to Recast).

use super::shape::{Ref, with_sim};
use crate::containers;
use crate::math::{from_vec, to_vec};
use abi::slots::i_physics_shape as s;
use abi::structs::{BBox, Vector3};
use abi::{NativeRef, Registry, bind, class};
use physics::rapier3d::parry::shape::Shape as Parry;
use physics::{Pose, Vector};
use std::collections::HashSet;

const ROUND_SUBDIVISIONS: u32 = 12;

type Mesh = (Vec<Vector>, Vec<[u32; 3]>);

fn append(out: &mut Mesh, pose: &Pose, (vertices, triangles): Mesh) {
    let base = out.0.len() as u32;
    out.0.extend(vertices.into_iter().map(|v| pose.transform_point(v)));
    out.1.extend(triangles.into_iter().map(|t| t.map(|i| i + base)));
}

/// A sphere as real triangulates one: an octahedron with each edge cut in `n` (n = 10 gives
/// real's 402 vertices and 800 triangles; its vertex positions differ slightly).
fn octasphere(radius: f32, n: i32) -> Mesh {
    let mut index = std::collections::HashMap::new();
    let mut mesh = Mesh::default();
    let mut vertex = |p: [i32; 3], mesh: &mut Mesh| {
        *index.entry(p).or_insert_with(|| {
            mesh.0.push(Vector::new(p[0] as f32, p[1] as f32, p[2] as f32).normalize() * radius);
            mesh.0.len() as u32 - 1
        })
    };
    for s in 0..8 {
        let sign = [1, 2, 4].map(|bit| if s & bit == 0 { 1 } else { -1 });
        let at = |i: i32, j: i32| [sign[0] * (n - i - j), sign[1] * i, sign[2] * j];
        for i in 0..n {
            for j in 0..n - i {
                let mut tris = vec![[at(i, j), at(i + 1, j), at(i, j + 1)]];
                if i + j + 1 < n {
                    tris.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                }
                for t in tris {
                    let [a, b, c] = t.map(|p| vertex(p, &mut mesh));
                    let [pa, pb, pc] = [a, b, c].map(|v| mesh.0[v as usize]);
                    let outward = (pb - pa).cross(pc - pa).dot(pa + pb + pc) > 0.0;
                    mesh.1.push(if outward { [a, b, c] } else { [a, c, b] });
                }
            }
        }
    }
    mesh
}

fn triangles_of(shape: &dyn Parry, pose: &Pose, out: &mut Mesh) {
    let n = ROUND_SUBDIVISIONS;
    let mesh = if let Some(b) = shape.as_ball() {
        octasphere(b.radius, 10)
    } else if let Some(c) = shape.as_cuboid() {
        c.to_trimesh()
    } else if let Some(c) = shape.as_capsule() {
        c.to_trimesh(n, n / 2)
    } else if let Some(c) = shape.as_cylinder() {
        c.to_trimesh(n)
    } else if let Some(c) = shape.as_cone() {
        c.to_trimesh(n)
    } else if let Some(h) = shape.as_convex_polyhedron() {
        h.to_trimesh()
    } else if let Some(h) = shape.as_heightfield() {
        h.to_trimesh()
    } else if let Some(m) = shape.as_trimesh() {
        (m.vertices().to_vec(), m.indices().to_vec())
    } else if let Some(c) = shape.as_compound() {
        for (part, sub) in c.shapes() {
            triangles_of(sub.as_ref(), &(*pose * *part), out);
        }
        return;
    } else {
        return;
    };
    append(out, pose, mesh);
}

/// The shape's triangles, in its body's space or in world space.
fn triangulate(r: Ref, world: bool) -> Mesh {
    with_sim(r, |sim, id| {
        let Some(c) = sim.shape(id) else { return Mesh::default() };
        let pose = if world { *c.position() } else { c.position_wrt_parent().copied().unwrap_or(*c.position()) };
        let mut out = Mesh::default();
        triangles_of(c.shape(), &pose, &mut out);
        out
    })
}

fn touches(b: &BBox, t: [Vector; 3]) -> bool {
    let (lo, hi) = (to_vec(b.mins), to_vec(b.maxs));
    let min = t[0].min(t[1]).min(t[2]);
    let max = t[0].max(t[1]).max(t[2]);
    min.cmple(hi).all() && max.cmpge(lo).all()
}

fn write(vertices: NativeRef<class::CUtlVector_Vector_>, indices: NativeRef<class::CUtlVector_uint32_>, (v, t): Mesh) {
    containers::with(vertices, |out: &mut Vec<Vector3>| out.extend(v.into_iter().map(from_vec)));
    containers::with(indices, |out: &mut Vec<u32>| out.extend(t.into_iter().flatten()));
}

/// Each triangle edge once, as line-segment endpoint pairs.
fn outline((v, t): Mesh) -> Vec<Vector3> {
    let mut seen = HashSet::new();
    let edges = t.iter().flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)]);
    edges.filter(|&(a, b)| seen.insert((a.min(b), a.max(b)))).flat_map(|(a, b)| [from_vec(v[a as usize]), from_vec(v[b as usize])]).collect()
}

pub fn register(r: &mut Registry) {
    bind!(r, s::GET_TRIANGULATION, |r, vertices, indices| write(vertices, indices, triangulate(r, false)));
    bind!(r, s::GET_TRIANGULATION_FOR_NAVMESH, |r, vertices, indices, bounds| {
        let (v, t) = triangulate(r, true);
        let t = t.into_iter().filter(|tri| touches(&bounds, tri.map(|i| v[i as usize]))).collect();
        write(vertices, indices, (v, t))
    });
    bind!(r, s::GET_OUTLINE, |r, lines| {
        let edges = outline(triangulate(r, false));
        containers::with(lines, |out: &mut Vec<Vector3>| out.extend(edges))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use physics::rapier3d::parry::shape::Cuboid;

    #[test]
    fn a_box_has_twelve_triangles_and_twelve_edges() {
        let mut mesh = Mesh::default();
        let pose = Pose::from_parts(Vector::new(10.0, 0.0, 0.0), physics::Rotation::IDENTITY);
        triangles_of(&Cuboid::new(Vector::splat(1.0)), &pose, &mut mesh);
        assert_eq!((mesh.0.len(), mesh.1.len()), (8, 12));
        assert!(mesh.0.iter().all(|v| (9.0..=11.0).contains(&v.x)), "posed");
        let bounds = BBox { mins: crate::math::vec3(10.5, -5.0, -5.0), maxs: crate::math::vec3(20.0, 5.0, 5.0) };
        let touching = mesh.1.iter().filter(|t| touches(&bounds, t.map(|i| mesh.0[i as usize]))).count();
        assert!(touching > 0 && touching < 12);
        assert_eq!(outline(mesh).len(), 2 * 18, "12 box edges + 6 face diagonals");
    }

    #[test]
    fn a_sphere_has_reals_vertex_and_triangle_counts() {
        let mut mesh = Mesh::default();
        triangles_of(&physics::rapier3d::parry::shape::Ball::new(8.0), &Pose::IDENTITY, &mut mesh);
        assert_eq!((mesh.0.len(), mesh.1.len()), (402, 800));
        assert!(mesh.0.iter().all(|v| (v.length() - 8.0).abs() < 1e-4));
        let outward = |t: &[u32; 3]| {
            let [a, b, c] = t.map(|i| mesh.0[i as usize]);
            (b - a).cross(c - a).dot(a + b + c) > 0.0
        };
        assert!(mesh.1.iter().all(outward));
    }
}
