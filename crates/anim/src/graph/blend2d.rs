//! Blend2D weights: barycentric weights in the triangle holding the parameter point, or on the
//! closest point of any triangle when it lies outside them all.

use glam::Vec2;

fn barycentric(p: Vec2, [a, b, c]: [Vec2; 3]) -> Option<[f32; 3]> {
    let (v0, v1, v2) = (b - a, c - a, p - a);
    let d = v0.perp_dot(v1);
    if d.abs() < 1e-8 {
        return None;
    }
    let v = v2.perp_dot(v1) / d;
    let w = v0.perp_dot(v2) / d;
    Some([1.0 - v - w, v, w])
}

fn on_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    ((a + ab * t).distance_squared(p), t)
}

fn in_circumcircle(a: Vec2, b: Vec2, c: Vec2, p: Vec2) -> bool {
    let (a, b, c) = (a - p, b - p, c - p);
    let det = (a.length_squared()) * b.perp_dot(c) - b.length_squared() * a.perp_dot(c) + c.length_squared() * a.perp_dot(b);
    // Orientation-independent: flip by the triangle's winding.
    det * (b - a).perp_dot(c - a) > 0.0
}

/// Bowyer-Watson Delaunay triangulation, as managed's `GraphBuilder.TriangulateBlendSpace`
/// bakes it; empty for fewer than three points or all-collinear ones.
pub fn triangulate(points: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let mut v: Vec<Vec2> = points.iter().map(|&p| Vec2::from(p)).collect();
    let (min, max) = v.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let (center, r) = ((min + max) * 0.5, (max - min).max_element() * 16.0 + 1.0);
    v.extend([center + Vec2::new(-r, -r), center + Vec2::new(r, -r), center + Vec2::new(0.0, r)]);
    let mut tris = vec![[n, n + 1, n + 2]];
    for i in 0..n {
        let (bad, keep): (Vec<_>, Vec<_>) = tris.into_iter().partition(|t: &[usize; 3]| in_circumcircle(v[t[0]], v[t[1]], v[t[2]], v[i]));
        tris = keep;
        let edges: Vec<(usize, usize)> = bad.iter().flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect();
        let boundary = edges.iter().filter(|&&(a, b)| !edges.contains(&(b, a)));
        tris.extend(boundary.map(|&(a, b)| [a, b, i]).collect::<Vec<_>>());
    }
    tris.retain(|t| t.iter().all(|&i| i < n) && (v[t[1]] - v[t[0]]).perp_dot(v[t[2]] - v[t[0]]).abs() > 1e-6);
    tris
}

/// (point index, weight) pairs summing to 1; the nearest point alone without triangles.
pub fn weights(p: [f32; 2], points: &[[f32; 2]], triangles: &[[usize; 3]]) -> Vec<(usize, f32)> {
    let p = Vec2::from(p);
    let at = |i: usize| Vec2::from(points[i]);
    let valid = triangles.iter().filter(|t| t.iter().all(|&i| i < points.len()));
    let mut best: Option<(f32, Vec<(usize, f32)>)> = None;
    for t in valid {
        let corners = t.map(at);
        if let Some(w) = barycentric(p, corners).filter(|w| w.iter().all(|&x| x >= -1e-5)) {
            return t.iter().zip(w).map(|(&i, w)| (i, w.max(0.0))).collect();
        }
        for (e0, e1) in [(0, 1), (1, 2), (2, 0)] {
            let (d, s) = on_segment(p, corners[e0], corners[e1]);
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, vec![(t[e0], 1.0 - s), (t[e1], s)]));
            }
        }
    }
    if let Some((_, w)) = best {
        return w;
    }
    let nearest = (0..points.len()).min_by(|&a, &b| at(a).distance_squared(p).total_cmp(&at(b).distance_squared(p)));
    nearest.map(|i| vec![(i, 1.0)]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::weights;

    const SQUARE: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    const TRIS: [[usize; 3]; 2] = [[0, 1, 2], [0, 2, 3]];

    fn of(w: &[(usize, f32)], i: usize) -> f32 {
        w.iter().filter(|p| p.0 == i).map(|p| p.1).sum()
    }

    #[test]
    fn inside_points_get_barycentric_weights() {
        let w = weights([0.75, 0.25], &SQUARE, &TRIS);
        assert!((of(&w, 0) - 0.25).abs() < 1e-5 && (of(&w, 1) - 0.5).abs() < 1e-5 && (of(&w, 2) - 0.25).abs() < 1e-5, "{w:?}");
        assert!((weights([1.0, 1.0], &SQUARE, &TRIS).iter().map(|p| p.1).sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn outside_points_clamp_to_the_nearest_edge() {
        let w = weights([2.0, 0.5], &SQUARE, &TRIS);
        assert!((of(&w, 1) - 0.5).abs() < 1e-5 && (of(&w, 2) - 0.5).abs() < 1e-5, "{w:?}");
        assert_eq!(weights([5.0, 5.0], &SQUARE, &[]), [(2, 1.0)]);
    }

    #[test]
    fn delaunay_covers_the_points_hull() {
        let star = [[0.0, 0.0], [0.0, 1000.0], [750.0, 750.0], [1000.0, 0.0], [750.0, -750.0], [0.0, -1000.0], [-750.0, -750.0], [-1000.0, 0.0], [-750.0, 750.0]];
        let tris = super::triangulate(&star);
        let area = |t: &[usize; 3]| (super::Vec2::from(star[t[1]]) - super::Vec2::from(star[t[0]])).perp_dot(super::Vec2::from(star[t[2]]) - super::Vec2::from(star[t[0]])).abs() / 2.0;
        let octagon = 4.0 * 1000.0 * 750.0;
        assert!((tris.iter().map(area).sum::<f32>() - octagon).abs() < 1.0, "the triangles tile the hull: {tris:?}");
        assert!((0..star.len()).all(|i| tris.iter().any(|t| t.contains(&i))), "every point is used");
        let w = weights([500.0, 100.0], &star, &tris);
        assert!((w.iter().map(|p| p.1).sum::<f32>() - 1.0).abs() < 1e-5 && w.len() == 3, "{w:?}");
        assert!(super::triangulate(&[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]).is_empty(), "collinear");
    }
}
