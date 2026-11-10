//! ModelDoc's `RenderPrimitive*` nodes as meshes. Z-up; a triangle's front face winds
//! counter-clockwise about its outward normal.

use super::Vertex;

type V3 = [f32; 3];

/// Source's QAngle (pitch, yaw, roll in degrees) as a matrix: Rz(yaw) · Ry(pitch) · Rx(roll).
pub fn rotation([pitch, yaw, roll]: V3) -> [V3; 3] {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    [
        [cy * cp, cy * sp * sr - sy * cr, cy * sp * cr + sy * sr],
        [sy * cp, sy * sp * sr + cy * cr, sy * sp * cr - cy * sr],
        [-sp, cp * sr, cp * cr],
    ]
}

fn mul(m: &[V3; 3], v: V3) -> V3 {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    a.map(|x| x * s)
}

/// Mesh data being built; `place` moves it to the node's origin and angles.
#[derive(Default)]
pub struct Builder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Builder {
    /// A `segs_u` × `segs_v` grid from `corner`, spanning `u` and `v`, facing `u × v`.
    pub fn grid(&mut self, corner: V3, u: V3, v: V3, segs: (u32, u32), max_uv: (f32, f32)) {
        let (su, sv) = (segs.0.max(1), segs.1.max(1));
        let normal = normalize(cross(u, v));
        let base = self.vertices.len() as u32;
        for j in 0..=sv {
            for i in 0..=su {
                let (fu, fv) = (i as f32 / su as f32, j as f32 / sv as f32);
                let position = add(corner, add(scale(u, fu), scale(v, fv)));
                self.vertices.push(Vertex { position, normal, uv: [fu * max_uv.0, (1.0 - fv) * max_uv.1] });
            }
        }
        for j in 0..sv {
            for i in 0..su {
                let a = base + j * (su + 1) + i;
                let (b, c, d) = (a + 1, a + su + 1, a + su + 2);
                self.indices.extend([a, b, d, a, d, c]);
            }
        }
    }

    pub fn place(&mut self, origin: V3, angles: V3) {
        let m = rotation(angles);
        for v in &mut self.vertices {
            v.position = add(mul(&m, v.position), origin);
            v.normal = mul(&m, v.normal);
        }
    }
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: V3) -> V3 {
    let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if len > 0.0 { scale(a, 1.0 / len) } else { a }
}

/// A box of `size`, centred on the origin, `segs` subdivisions per axis.
pub fn cube(b: &mut Builder, size: V3, segs: [u32; 3], max_uv: (f32, f32)) {
    let h = scale(size, 0.5);
    let [x, y, z] = [[size[0], 0.0, 0.0], [0.0, size[1], 0.0], [0.0, 0.0, size[2]]];
    let neg = |a: V3| scale(a, -1.0);
    let [sx, sy, sz] = segs;
    // Each face: (corner, u, v) with u × v outward.
    b.grid([-h[0], -h[1], h[2]], x, y, (sx, sy), max_uv);
    b.grid([-h[0], h[1], -h[2]], x, neg(y), (sx, sy), max_uv);
    b.grid([h[0], -h[1], -h[2]], y, z, (sy, sz), max_uv);
    b.grid([-h[0], h[1], -h[2]], neg(y), z, (sy, sz), max_uv);
    b.grid([-h[0], -h[1], -h[2]], x, z, (sx, sz), max_uv);
    b.grid([h[0], h[1], -h[2]], neg(x), z, (sx, sz), max_uv);
}

/// A `width` × `height` plane in XY, centred on the origin, facing +Z.
pub fn plane(b: &mut Builder, width: f32, height: f32, segs: (u32, u32), max_uv: (f32, f32)) {
    b.grid([-width / 2.0, -height / 2.0, 0.0], [width, 0.0, 0.0], [0.0, height, 0.0], segs, max_uv);
}

/// A UV sphere: `segs_u` around Z, `segs_v` from the +Z pole to the -Z pole.
pub fn sphere(b: &mut Builder, radius: f32, segs_u: u32, segs_v: u32) {
    let (su, sv) = (segs_u.max(3), segs_v.max(2));
    for j in 0..=sv {
        let (st, ct) = (std::f32::consts::PI * j as f32 / sv as f32).sin_cos();
        for i in 0..=su {
            let (sp, cp) = (std::f32::consts::TAU * i as f32 / su as f32).sin_cos();
            let normal = [st * cp, st * sp, ct];
            b.vertices.push(Vertex { position: scale(normal, radius), normal, uv: [i as f32 / su as f32, j as f32 / sv as f32] });
        }
    }
    for j in 0..sv {
        for i in 0..su {
            let a = j * (su + 1) + i;
            let (n, c) = (a + 1, a + su + 1);
            b.indices.extend([a, c, n, n, c, c + 1]);
        }
    }
}
