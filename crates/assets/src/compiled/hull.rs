//! Writing a convex hull as `RnHull_t` the way real engine2's compiler stores it: vertex
//! positions, face planes, a half-edge mesh (twins in adjacent slots), and mass properties of
//! the solid at unit density. Hulls are small (tens of points), so faces come from brute force.

use super::kv::{floats, int_value, list, obj};
use ::kv::Value;

const EPS: f32 = 1e-3;

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn scale(a: V3, s: f32) -> V3 {
    a.map(|x| x * s)
}

fn unit(a: V3) -> Option<V3> {
    let len = dot(a, a).sqrt();
    (len > 1e-6).then(|| scale(a, 1.0 / len))
}

/// Each outward face plane (normal, offset) with its vertices counter-clockwise about the normal.
fn faces(points: &[V3]) -> Vec<(V3, f32, Vec<usize>)> {
    let mut out: Vec<(V3, f32, Vec<usize>)> = Vec::new();
    let n = points.len();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let Some(mut normal) = unit(cross(sub(points[j], points[i]), sub(points[k], points[i]))) else { continue };
                let side = |n: V3| points.iter().all(|&p| dot(n, sub(p, points[i])) <= EPS);
                if !side(normal) {
                    normal = scale(normal, -1.0);
                    if !side(normal) {
                        continue;
                    }
                }
                let offset = dot(normal, points[i]);
                if out.iter().any(|(m, o, _)| dot(*m, normal) > 1.0 - 1e-5 && (o - offset).abs() < EPS) {
                    continue;
                }
                let on: Vec<usize> = (0..n).filter(|&v| (dot(normal, points[v]) - offset).abs() <= EPS).collect();
                out.push((normal, offset, around(points, normal, on)));
            }
        }
    }
    out
}

/// `on` sorted counter-clockwise about `normal`, points in the middle of an edge dropped.
fn around(points: &[V3], normal: V3, mut on: Vec<usize>) -> Vec<usize> {
    let c = scale(on.iter().fold([0.0; 3], |s, &v| [s[0] + points[v][0], s[1] + points[v][1], s[2] + points[v][2]]), 1.0 / on.len() as f32);
    let u = unit(sub(points[on[0]], c)).unwrap_or([1.0, 0.0, 0.0]);
    let w = cross(normal, u);
    let angle = |v: usize| dot(sub(points[v], c), w).atan2(dot(sub(points[v], c), u));
    on.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
    let m = on.len();
    (0..m)
        .filter(|&i| {
            let (a, b, d) = (points[on[(i + m - 1) % m]], points[on[i]], points[on[(i + 1) % m]]);
            dot(cross(sub(b, a), sub(d, b)), normal) > 1e-6
        })
        .map(|i| on[i])
        .collect()
}

/// Volume, centre of mass and inertia tensor (about the centre) of the solid, density 1.
fn mass(points: &[V3], faces: &[(V3, f32, Vec<usize>)]) -> (f32, V3, [[f32; 3]; 3]) {
    let r = scale(points.iter().fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]), 1.0 / points.len() as f32);
    let (mut volume, mut first, mut second) = (0.0f64, [0.0f64; 3], [[0.0f64; 3]; 3]);
    for (_, _, f) in faces {
        for i in 1..f.len() - 1 {
            let tet = [r, points[f[0]], points[f[i]], points[f[i + 1]]];
            let v = f64::from(dot(sub(tet[1], r), cross(sub(tet[2], r), sub(tet[3], r)))) / 6.0;
            let sum: [f64; 3] = std::array::from_fn(|a| tet.iter().map(|p| f64::from(p[a])).sum());
            volume += v;
            (0..3).for_each(|a| first[a] += v * sum[a] / 4.0);
            // ∫ x_a x_b dV over a tetrahedron = V/20 (Σ p_a p_b + S_a S_b).
            for (a, row) in second.iter_mut().enumerate() {
                for (b, x) in row.iter_mut().enumerate() {
                    *x += v / 20.0 * (tet.iter().map(|p| f64::from(p[a]) * f64::from(p[b])).sum::<f64>() + sum[a] * sum[b]);
                }
            }
        }
    }
    let c: [f64; 3] = first.map(|x| x / volume);
    let central: [[f64; 3]; 3] = std::array::from_fn(|a| std::array::from_fn(|b| second[a][b] - volume * c[a] * c[b]));
    let trace = central[0][0] + central[1][1] + central[2][2];
    let inertia = std::array::from_fn(|a| std::array::from_fn(|b| ((if a == b { trace } else { 0.0 }) - central[a][b]) as f32));
    (volume as f32, c.map(|x| x as f32), inertia)
}

/// `RnHull_t` for the convex hull of `points`.
pub fn write(points: &[V3]) -> Value {
    let faces = faces(points);
    let mut used: Vec<usize> = faces.iter().flat_map(|(_, _, f)| f.iter().copied()).collect();
    used.sort_unstable();
    used.dedup();
    let index = |v: usize| used.iter().position(|&u| u == v).unwrap() as i64;
    // Half-edges: (origin, face, next); a→b gets slot 2k and b→a 2k+1.
    let mut edges: Vec<(usize, usize, usize)> = Vec::new();
    let mut slot = std::collections::HashMap::new();
    let mut first = Vec::new();
    for (fi, (_, _, f)) in faces.iter().enumerate() {
        let ids: Vec<usize> = (0..f.len())
            .map(|i| {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                let e = match slot.get(&(a, b)) {
                    Some(&e) => e,
                    None => {
                        let e = edges.len();
                        edges.extend([(a, usize::MAX, 0), (b, usize::MAX, 0)]);
                        slot.insert((a, b), e);
                        slot.insert((b, a), e + 1);
                        e
                    }
                };
                edges[e].1 = fi;
                e
            })
            .collect();
        for (i, &e) in ids.iter().enumerate() {
            edges[e].2 = ids[(i + 1) % ids.len()];
        }
        first.push(ids[0]);
    }
    let (volume, c, inertia) = mass(points, &faces);
    let radius = used.iter().map(|&v| dot(sub(points[v], c), sub(points[v], c)).sqrt()).fold(0.0, f32::max);
    let thickness = faces.iter().map(|(n, o, _)| o - dot(*n, c)).fold(f32::MAX, f32::min);
    let lo: V3 = std::array::from_fn(|a| used.iter().map(|&v| points[v][a]).fold(f32::MAX, f32::min));
    let hi: V3 = std::array::from_fn(|a| used.iter().map(|&v| points[v][a]).fold(f32::MIN, f32::max));
    let mass_properties: Vec<f32> = (0..3).flat_map(|r| [inertia[r][0], inertia[r][1], inertia[r][2], c[r]]).collect();
    let edge = |&(origin, face, next): &(usize, usize, usize), e: usize| {
        obj(vec![("m_nNext", int_value(next as i64)), ("m_nTwin", int_value((e ^ 1) as i64)), ("m_nOrigin", int_value(index(origin))), ("m_nFace", int_value(face as i64))])
    };
    obj(vec![
        ("m_vCentroid", floats(&c)),
        ("m_flMaxAngularRadius", Value::Double(radius.into())),
        ("m_Vertices", list(used.iter().map(|&v| floats(&points[v])))),
        ("m_Planes", list(faces.iter().map(|(n, o, _)| obj(vec![("m_vNormal", floats(n)), ("m_flOffset", Value::Double((*o).into()))])))),
        ("m_Edges", list(edges.iter().enumerate().map(|(e, x)| edge(x, e)))),
        ("m_Faces", list(first.iter().map(|&e| obj(vec![("m_nEdge", int_value(e as i64))])))),
        ("m_vOrthographicAreas", floats(&[0.25; 3])),
        ("m_MassProperties", floats(&mass_properties)),
        ("m_flVolume", Value::Double(volume.into())),
        ("m_flMaxMotionRadius", Value::Double(radius.into())),
        ("m_flMinMotionThickness", Value::Double(thickness.into())),
        ("m_Bounds", obj(vec![("m_vMinBounds", floats(&lo)), ("m_vMaxBounds", floats(&hi))])),
        ("m_nFlags", int_value(0)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::kv::Object;

    fn get<'a>(o: &'a Value, k: &str) -> &'a Value {
        o.as_object().and_then(|o: &Object| o.get(k)).unwrap()
    }

    /// Real's compiled 20-unit box (models/editor/air_node) has these numbers.
    #[test]
    fn a_box_matches_real_mass_properties() {
        let pts = crate::compiled::fixture::corners([-10.0; 3], [10.0; 3]);
        let h = write(&pts);
        assert_eq!(get(&h, "m_Faces").as_array().unwrap().len(), 6);
        assert_eq!(get(&h, "m_Edges").as_array().unwrap().len(), 24);
        assert!((get(&h, "m_flVolume").as_f64().unwrap() - 8000.0).abs() < 0.1);
        assert!((get(&h, "m_flMinMotionThickness").as_f64().unwrap() - 10.0).abs() < 1e-3);
        assert!((get(&h, "m_flMaxAngularRadius").as_f64().unwrap() - 17.3205).abs() < 1e-3);
        let m: Vec<f64> = get(&h, "m_MassProperties").as_array().unwrap().iter().filter_map(Value::as_f64).collect();
        assert!((m[0] - 533_333.3).abs() < 1.0 && (m[5] - 533_333.3).abs() < 1.0 && m[1].abs() < 1e-3, "{m:?}");
        let edges = get(&h, "m_Edges").as_array().unwrap();
        for (e, x) in edges.iter().enumerate() {
            let next = get(x, "m_nNext").as_f64().unwrap() as usize;
            assert_eq!(get(&edges[next], "m_nFace").as_f64(), get(x, "m_nFace").as_f64(), "edge {e}'s next is on its face");
            let twin = &edges[e ^ 1];
            assert_eq!(get(twin, "m_nOrigin").as_f64(), get(&edges[next], "m_nOrigin").as_f64(), "edge {e}'s twin starts where it ends");
        }
    }
}
