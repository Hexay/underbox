//! `PhysicsHullSimplificationParams_t` (managed `HullSimplify`): iterative vertex removal. Each
//! step drops the vertex whose removal moves the surface least, while the hull is over a vertex,
//! face or edge cap or that move is within the distance tolerance. Every method but `None` runs
//! this; the angle tolerance isn't used.

use super::hull::Hull;
use glam::Vec3;

/// managed's `SimplifyMethod.None`.
pub const NONE: i32 = 2;

#[derive(Debug, Clone, Copy, Default)]
pub struct Params {
    pub inches: f32,
    pub max_faces: i32,
    pub max_edges: i32,
    pub max_verts: i32,
    pub method: i32,
}

/// Faces after coplanar triangles merge (Euler: V − E + F = 2).
fn faces(h: &Hull) -> usize {
    (h.edges.len() + 2).saturating_sub(h.vertices.len())
}

fn over(cap: i32, n: usize) -> bool {
    cap > 0 && n > cap as usize
}

/// How far outside `h` the point lies (its largest face-plane distance).
fn outside(h: &Hull, p: Vec3) -> f32 {
    h.triangles.iter().map(|t| {
        let [a, b, c] = t.map(|i| h.vertices[i as usize]);
        (b - a).cross(c - a).normalize_or_zero().dot(p - a)
    }).fold(0.0, f32::max)
}

/// The hull without its vertex `i`, and how far that vertex sat outside it.
fn without(h: &Hull, i: usize) -> Option<(f32, Hull)> {
    let rest: Vec<Vec3> = h.vertices.iter().enumerate().filter(|&(k, _)| k != i).map(|(_, v)| *v).collect();
    let r = Hull::from_points(&rest)?;
    Some((outside(&r, h.vertices[i]), r))
}

pub fn simplify(hull: Hull, p: Params) -> Hull {
    if p.method == NONE {
        return hull;
    }
    let mut h = hull;
    while h.vertices.len() > 4 {
        let capped = over(p.max_verts, h.vertices.len()) || over(p.max_edges, h.edges.len()) || over(p.max_faces, faces(&h));
        let best = (0..h.vertices.len()).filter_map(|i| without(&h, i)).min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((d, r)) if capped || d <= p.inches => h = r,
            _ => break,
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube with a slightly raised point on its top face.
    fn bumpy() -> Hull {
        let mut pts: Vec<Vec3> = (0..8).map(|i| Vec3::new([-1.0, 1.0][i & 1], [-1.0, 1.0][(i >> 1) & 1], [-1.0, 1.0][i >> 2]) * 10.0).collect();
        pts.push(Vec3::new(0.0, 0.0, 10.5));
        Hull::from_points(&pts).unwrap()
    }

    #[test]
    fn caps_and_tolerance_remove_the_least_significant_vertices() {
        let h = bumpy();
        assert_eq!(h.vertices.len(), 9);
        let p = Params { method: 1, ..Params::default() };
        assert_eq!(simplify(h.clone(), Params { method: NONE, inches: 100.0, ..p }).vertices.len(), 9, "None keeps the exact hull");
        assert_eq!(simplify(h.clone(), p).vertices.len(), 9, "no caps, no tolerance");
        let cube = simplify(h.clone(), Params { inches: 1.0, ..p });
        assert_eq!((cube.vertices.len(), cube.edges.len(), faces(&cube)), (8, 12, 6), "the 0.5 bump is within 1 inch");
        assert!(!cube.vertices.iter().any(|v| v.z > 10.0));
        assert_eq!(simplify(h.clone(), Params { max_verts: 8, ..p }).vertices.len(), 8);
        assert_eq!(simplify(h.clone(), Params { max_faces: 6, ..p }).vertices.len(), 8, "the bump's 4 faces merge back into 1");
        let few = simplify(h, Params { max_verts: 5, ..p });
        assert_eq!(few.vertices.len(), 5);
        assert!(few.volume() > 0.0);
    }
}
