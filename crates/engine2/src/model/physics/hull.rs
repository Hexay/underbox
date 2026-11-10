//! Convex hulls (`RnHull_t`): the hull of a point cloud with its faces' edges, volume, centroid
//! and area. Triangulated faces are merged back, so a box has 12 edges, not 18.

use glam::Vec3;
use physics::rapier3d::parry::transformation::convex_hull;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hull {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Vertex pairs, once per edge between non-coplanar faces.
    pub edges: Vec<[u32; 2]>,
}

fn normal(v: &[Vec3], t: &[u32; 3]) -> Vec3 {
    let p = t.map(|i| v[i as usize]);
    (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero()
}

impl Hull {
    pub fn from_points(points: &[Vec3]) -> Option<Self> {
        if points.len() < 4 {
            return None;
        }
        let input: Vec<physics::Vector> = points.iter().map(|p| physics::Vector::new(p.x, p.y, p.z)).collect();
        let (vertices, triangles) = convex_hull(&input);
        if triangles.is_empty() {
            return None;
        }
        let vertices: Vec<Vec3> = vertices.iter().map(|p| Vec3::new(p.x, p.y, p.z)).collect();
        let edges = feature_edges(&vertices, &triangles);
        Some(Self { vertices, triangles, edges })
    }

    pub fn bbox(&self) -> (Vec3, Vec3) {
        let lo = self.vertices.iter().fold(Vec3::splat(f32::MAX), |a, b| a.min(*b));
        let hi = self.vertices.iter().fold(Vec3::splat(f32::MIN), |a, b| a.max(*b));
        if self.vertices.is_empty() { (Vec3::ZERO, Vec3::ZERO) } else { (lo, hi) }
    }

    /// Signed tetrahedra from the origin: (volume, volume-weighted centroid).
    fn moments(&self) -> (f32, Vec3) {
        self.triangles.iter().fold((0.0, Vec3::ZERO), |(vol, c), t| {
            let [a, b, d] = t.map(|i| self.vertices[i as usize]);
            let v = a.dot(b.cross(d)) / 6.0;
            (vol + v, c + (a + b + d) * (v / 4.0))
        })
    }

    pub fn volume(&self) -> f32 {
        self.moments().0.abs()
    }

    /// The solid's centre of mass at uniform density.
    pub fn mass_center(&self) -> Vec3 {
        let (v, c) = self.moments();
        if v.abs() < f32::EPSILON { self.centroid() } else { c / v }
    }

    /// The mean of the vertices.
    pub fn centroid(&self) -> Vec3 {
        self.vertices.iter().sum::<Vec3>() / self.vertices.len().max(1) as f32
    }

    pub fn surface_area(&self) -> f32 {
        self.triangles.iter().map(|t| {
            let [a, b, c] = t.map(|i| self.vertices[i as usize]);
            (b - a).cross(c - a).length() * 0.5
        }).sum()
    }

    pub fn transformed(&self, f: impl Fn(Vec3) -> Vec3) -> Self {
        Self { vertices: self.vertices.iter().copied().map(f).collect(), ..self.clone() }
    }
}

/// Edges where the faces either side aren't coplanar.
fn feature_edges(v: &[Vec3], triangles: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut faces: std::collections::HashMap<[u32; 2], Vec<Vec3>> = Default::default();
    for t in triangles {
        let n = normal(v, t);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            faces.entry([a.min(b), a.max(b)]).or_default().push(n);
        }
    }
    let mut edges: Vec<[u32; 2]> = faces.into_iter().filter(|(_, n)| n.len() != 2 || n[0].dot(n[1]) < 0.9999).map(|(e, _)| e).collect();
    edges.sort_unstable();
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_hull_has_box_measures() {
        let corners: Vec<Vec3> = (0..8).map(|i| Vec3::new(if i & 1 != 0 { 2.0 } else { 0.0 }, if i & 2 != 0 { 4.0 } else { 0.0 }, if i & 4 != 0 { 6.0 } else { 0.0 })).collect();
        let h = Hull::from_points(&corners).unwrap();
        assert_eq!(h.vertices.len(), 8);
        assert_eq!(h.edges.len(), 12);
        assert!((h.volume() - 48.0).abs() < 1e-3);
        assert!((h.surface_area() - 88.0).abs() < 1e-3);
        assert!((h.mass_center() - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        assert!(Hull::from_points(&corners[..3]).is_none());
    }
}
