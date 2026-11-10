//! Polygon and triangle helpers behind `MeshGlue` (triangulation, segment clipping) and
//! `CModel::MeshTrace` (ray against triangles). Polygons may be non-planar; they're projected
//! onto the plane of their Newell normal.

use glam::{Vec2, Vec3};

/// The polygon's area-weighted normal (Newell's method); zero for a degenerate polygon.
pub fn newell_normal(poly: &[Vec3]) -> Vec3 {
    let n = poly.iter().zip(poly.iter().cycle().skip(1)).fold(Vec3::ZERO, |n, (a, b)| n + Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y)));
    n.normalize_or_zero()
}

/// Two axes spanning the plane with normal `n`, right-handed about it.
pub fn plane_basis(n: Vec3) -> (Vec3, Vec3) {
    let u = n.any_orthonormal_vector();
    (u, n.cross(u))
}

fn project(poly: &[Vec3], n: Vec3) -> Vec<Vec2> {
    let (u, v) = plane_basis(n);
    poly.iter().map(|p| Vec2::new(p.dot(u), p.dot(v))).collect()
}

fn cross2(a: Vec2, b: Vec2) -> f32 {
    a.perp_dot(b)
}

fn inside_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let (d1, d2, d3) = (cross2(b - a, p - a), cross2(c - b, p - b), cross2(a - c, p - c));
    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
}

/// Ear-clip a simple polygon into triangles (indices into `poly`), wound as the polygon is.
pub fn triangulate(poly: &[Vec3]) -> Vec<[usize; 3]> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let p = project(poly, newell_normal(poly));
    let mut left: Vec<usize> = (0..poly.len()).collect();
    let mut out = Vec::with_capacity(poly.len() - 2);
    while left.len() > 3 {
        let n = left.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (left[(i + n - 1) % n], left[i], left[(i + 1) % n]);
            cross2(p[b] - p[a], p[c] - p[b]) > 0.0 && !left.iter().any(|&j| j != a && j != b && j != c && inside_triangle(p[j], p[a], p[b], p[c]))
        });
        // A degenerate or self-touching polygon has no clean ear: fan out what's left.
        let i = ear.unwrap_or(1);
        out.push([left[(i + n - 1) % n], left[i], left[(i + 1) % n]]);
        left.remove(i);
    }
    out.push([left[0], left[1], left[2]]);
    out
}

fn point_in_polygon(q: Vec2, p: &[Vec2]) -> bool {
    let mut inside = false;
    for (a, b) in p.iter().zip(p.iter().cycle().skip(1)) {
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The parts of segment `a`–`b` inside the polygon, as start/end point pairs, in order along it.
pub fn clip_segment(poly: &[Vec3], a: Vec3, b: Vec3) -> Vec<Vec3> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let n = newell_normal(poly);
    let p = project(poly, n);
    let ab = project(&[a, b], n);
    let (s, d) = (ab[0], ab[1] - ab[0]);
    let mut cuts = vec![0.0, 1.0];
    for (e0, e1) in p.iter().zip(p.iter().cycle().skip(1)) {
        let e = *e1 - *e0;
        let denom = cross2(d, e);
        if denom.abs() < 1e-9 {
            continue;
        }
        let t = cross2(*e0 - s, e) / denom;
        let u = cross2(*e0 - s, d) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            cuts.push(t);
        }
    }
    cuts.sort_by(f32::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        if point_in_polygon(s + d * ((w[0] + w[1]) * 0.5), &p) {
            out.extend([a.lerp(b, w[0]), a.lerp(b, w[1])]);
        }
    }
    out
}

/// A ray hit on a triangle: distance along the (unit) direction and barycentric (u, v).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriangleHit {
    pub distance: f32,
    pub u: f32,
    pub v: f32,
}

/// Möller–Trumbore, both faces; `dir` must be unit length.
pub fn ray_triangle(origin: Vec3, dir: Vec3, [a, b, c]: [Vec3; 3]) -> Option<TriangleHit> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let s = origin - a;
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    let distance = e2.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance >= 0.0).then_some(TriangleHit { distance, u, v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec3> {
        vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)]
    }

    #[test]
    fn triangulates_concave_polygon_without_leaving_it() {
        let l = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0)].map(|(x, y)| Vec3::new(x, y, 0.0));
        let tris = triangulate(&l);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris.iter().map(|t| (l[t[1]] - l[t[0]]).cross(l[t[2]] - l[t[0]]).z * 0.5).sum();
        assert!((area - 64.0).abs() < 1e-3, "{area}");
        assert_eq!(triangulate(&square()).len(), 2);
    }

    #[test]
    fn clips_segment_to_polygon() {
        let pts = clip_segment(&square(), Vec3::new(-5.0, 5.0, 0.0), Vec3::new(15.0, 5.0, 0.0));
        assert_eq!(pts.len(), 2);
        assert!((pts[0].x - 0.0).abs() < 1e-4 && (pts[1].x - 10.0).abs() < 1e-4, "{pts:?}");
        assert!(clip_segment(&square(), Vec3::new(-5.0, 20.0, 0.0), Vec3::new(15.0, 20.0, 0.0)).is_empty());
    }

    #[test]
    fn ray_hits_triangle_from_either_side() {
        let tri = [Vec3::ZERO, Vec3::X * 10.0, Vec3::Y * 10.0];
        let hit = ray_triangle(Vec3::new(1.0, 1.0, 5.0), -Vec3::Z, tri).unwrap();
        assert!((hit.distance - 5.0).abs() < 1e-5);
        assert!(ray_triangle(Vec3::new(1.0, 1.0, -5.0), Vec3::Z, tri).is_some());
        assert!(ray_triangle(Vec3::new(9.0, 9.0, 5.0), -Vec3::Z, tri).is_none());
    }
}
