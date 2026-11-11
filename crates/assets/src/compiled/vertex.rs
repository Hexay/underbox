//! Decoding vertex attributes by semantic and DXGI format into one plain vertex, including the two
//! compressed normal/tangent encodings (VBIB.cs `DecompressNormal`, `DecompressNormalTangents2`).

use super::vbib::{Attribute, Buffer};
use half::f16;

/// DXGI formats compiled vertex buffers use (DXGIFormat.cs).
pub mod dxgi {
    pub const R32G32B32A32_FLOAT: u32 = 2;
    pub const R32G32B32A32_SINT: u32 = 4;
    pub const R32G32B32_FLOAT: u32 = 6;
    pub const R16G16B16A16_FLOAT: u32 = 10;
    pub const R16G16B16A16_UNORM: u32 = 11;
    pub const R16G16B16A16_UINT: u32 = 12;
    pub const R16G16B16A16_SNORM: u32 = 13;
    pub const R16G16B16A16_SINT: u32 = 14;
    pub const R32G32_FLOAT: u32 = 16;
    pub const R8G8B8A8_UNORM: u32 = 28;
    pub const R8G8B8A8_UINT: u32 = 30;
    pub const R16G16_FLOAT: u32 = 34;
    pub const R16G16_UNORM: u32 = 35;
    pub const R16G16_SNORM: u32 = 37;
    pub const R16G16_SINT: u32 = 38;
    pub const R32_FLOAT: u32 = 41;
    pub const R32_UINT: u32 = 42;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// xyz and the bitangent sign.
    pub tangent: [f32; 4],
    pub uv: [[f32; 2]; 2],
    pub color: [u8; 4],
    pub bones: [u8; 4],
    pub weights: [f32; 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Self { position: [0.0; 3], normal: [0.0; 3], tangent: [0.0, 0.0, 0.0, 1.0], uv: [[0.0; 2]; 2], color: [255; 4], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0] }
    }
}

/// Up to four components of one attribute as floats (normalized formats scaled, integers as is).
pub fn components(bytes: &[u8], format: u32) -> Option<[f32; 4]> {
    use dxgi::*;
    let get = |n: usize, size: usize, f: &dyn Fn(&[u8]) -> f32| -> Option<[f32; 4]> {
        let mut out = [0.0; 4];
        for (i, o) in out.iter_mut().enumerate().take(n) {
            *o = f(bytes.get(i * size..(i + 1) * size)?);
        }
        Some(out)
    };
    let f32_ = |b: &[u8]| f32::from_le_bytes(b.try_into().unwrap());
    let f16_ = |b: &[u8]| f16::from_le_bytes([b[0], b[1]]).to_f32();
    let u16_ = |b: &[u8]| f32::from(u16::from_le_bytes([b[0], b[1]]));
    let i16_ = |b: &[u8]| f32::from(i16::from_le_bytes([b[0], b[1]]));
    match format {
        R32G32B32A32_FLOAT => get(4, 4, &f32_),
        R32G32B32_FLOAT => get(3, 4, &f32_),
        R32G32_FLOAT => get(2, 4, &f32_),
        R32_FLOAT => get(1, 4, &f32_),
        R16G16B16A16_FLOAT => get(4, 2, &f16_),
        R16G16_FLOAT => get(2, 2, &f16_),
        R8G8B8A8_UNORM => get(4, 1, &|b| f32::from(b[0]) / 255.0),
        R8G8B8A8_UINT => get(4, 1, &|b| f32::from(b[0])),
        R16G16B16A16_UNORM => get(4, 2, &|b| u16_(b) / 65535.0),
        R16G16_UNORM => get(2, 2, &|b| u16_(b) / 65535.0),
        R16G16B16A16_SNORM => get(4, 2, &|b| (i16_(b) / 32767.0).max(-1.0)),
        R16G16_SNORM => get(2, 2, &|b| (i16_(b) / 32767.0).max(-1.0)),
        R16G16B16A16_UINT | R16G16B16A16_SINT | R16G16_SINT => get(if format == R16G16_SINT { 2 } else { 4 }, 2, &u16_),
        R32G32B32A32_SINT => get(4, 4, &|b| u32::from_le_bytes(b.try_into().unwrap()) as f32),
        R32_UINT => get(1, 4, &|b| u32::from_le_bytes(b.try_into().unwrap()) as f32),
        _ => None,
    }
}

/// Version 1 compressed normals: two bytes fold a hemisphere octahedron with the z sign.
fn decompress_normal(x: u8, y: u8) -> [f32; 3] {
    let (mut x, mut y) = (f32::from(x) - 128.0, f32::from(y) - 128.0);
    let (zs, ts) = (f32::from(u8::from(x < 0.0)), f32::from(u8::from(y < 0.0)));
    let (z_sign, t_sign) = (1.0 - 2.0 * zs, 1.0 - 2.0 * ts);
    x = x * z_sign - zs - 64.0;
    y = y * t_sign - ts - 64.0;
    let (xs, ys) = (f32::from(u8::from(x < 0.0)), f32::from(u8::from(y < 0.0)));
    let (x_sign, y_sign) = (1.0 - 2.0 * xs, 1.0 - 2.0 * ys);
    x = (x * x_sign - xs) / 63.0;
    y = (y * y_sign - ys) / 63.0;
    let z = 1.0 - x - y;
    let len = (x * x + y * y + z * z).sqrt();
    [x / len * x_sign, y / len * y_sign, z / len * z_sign]
}

/// Version 2 (`R32_UINT`): an octahedral normal in 10+10 bits, the tangent as an 11-bit angle
/// around it, and the bitangent sign.
pub fn unpack_frame(p: u32) -> ([f32; 3], [f32; 4]) {
    let unit = |bits: u32| bits as f32 / 1023.0 * 2.0 - 1.0;
    let (mut x, mut y) = (unit((p >> 12) & 0x3ff), unit((p >> 22) & 0x3ff));
    let z = 1.0 - x.abs() - y.abs();
    let c = (-z).clamp(0.0, 1.0);
    x += if x >= 0.0 { -c } else { c };
    y += if y >= 0.0 { -c } else { c };
    let len = (x * x + y * y + z * z).sqrt();
    let n = [x / len, y / len, z / len];
    let s = if n[2] >= 0.0 { 1.0 } else { -1.0 };
    let r = 1.0 / (s + n[2]);
    let t0 = [-s * n[0] * n[0] * r + 1.0, -s * (n[0] * n[1] * r), -s * n[0]];
    let cross = [n[1] * t0[2] - n[2] * t0[1], n[2] * t0[0] - n[0] * t0[2], n[0] * t0[1] - n[1] * t0[0]];
    let a = ((p >> 1) & 0x7ff) as f32 / 2047.0 * std::f32::consts::TAU;
    let t: [f32; 3] = std::array::from_fn(|i| t0[i] * a.cos() + cross[i] * a.sin());
    (n, [t[0], t[1], t[2], if p & 1 == 0 { -1.0 } else { 1.0 }])
}

/// The inverse of [`unpack_frame`] (for fixtures): `tangent` must be perpendicular to `normal`.
pub fn pack_frame(normal: [f32; 3], tangent: [f32; 4]) -> u32 {
    let l1 = normal.iter().map(|v| v.abs()).sum::<f32>();
    let (mut x, mut y) = (normal[0] / l1, normal[1] / l1);
    if normal[2] < 0.0 {
        (x, y) = ((1.0 - y.abs()) * x.signum(), (1.0 - x.abs()) * y.signum());
    }
    let q = |v: f32| (((v + 1.0) / 2.0 * 1023.0).round() as u32).min(1023);
    let base = (q(y) << 22) | (q(x) << 12) | u32::from(tangent[3] > 0.0);
    let (n, t0) = unpack_frame(base);
    let t0 = [t0[0], t0[1], t0[2]];
    let cross = [n[1] * t0[2] - n[2] * t0[1], n[2] * t0[0] - n[0] * t0[2], n[0] * t0[1] - n[1] * t0[0]];
    let dot = |a: [f32; 3]| a[0] * tangent[0] + a[1] * tangent[1] + a[2] * tangent[2];
    let a = dot(cross).atan2(dot(t0)).rem_euclid(std::f32::consts::TAU);
    base | ((((a / std::f32::consts::TAU) * 2047.0).round() as u32 % 2048) << 1)
}

fn apply(v: &mut Vertex, a: &Attribute, bytes: &[u8]) {
    let Some(c) = components(bytes, a.format) else { return };
    let xyz = [c[0], c[1], c[2]];
    match a.semantic.as_str() {
        "POSITION" => v.position = xyz,
        "NORMAL" if a.format == dxgi::R8G8B8A8_UNORM => {
            v.normal = decompress_normal(bytes[0], bytes[1]);
            let t = decompress_normal(bytes[2], bytes[3]);
            v.tangent = [t[0], t[1], t[2], if bytes[3] < 128 { -1.0 } else { 1.0 }];
        }
        "NORMAL" if a.format == dxgi::R32_UINT => (v.normal, v.tangent) = unpack_frame(c[0] as u32),
        "NORMAL" => v.normal = xyz,
        "TANGENT" => v.tangent = c,
        "TEXCOORD" if (0..2).contains(&a.index) => v.uv[a.index as usize] = [c[0], c[1]],
        "COLOR" if a.index == 0 => v.color = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8),
        "BLENDINDICES" => v.bones = c.map(|x| x as u8),
        "BLENDWEIGHT" | "BLENDWEIGHTS" => v.weights = c,
        _ => {}
    }
}

/// The vertices of `streams` (one draw call's bound vertex buffers, all the same length).
pub fn decode(streams: &[&Buffer]) -> Vec<Vertex> {
    let count = streams.iter().map(|b| b.count).min().unwrap_or(0);
    let mut out = vec![Vertex::default(); count];
    for b in streams {
        for a in &b.attributes {
            for (i, v) in out.iter_mut().enumerate() {
                if let Some(bytes) = b.data.get(i * b.stride + a.offset as usize..(i + 1) * b.stride) {
                    apply(v, a, bytes);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_1_normals_fold_the_z_sign() {
        assert_eq!(decompress_normal(192, 192), [0.0, 0.0, 1.0]);
        assert_eq!(decompress_normal(63, 192), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn packed_frames_round_trip() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        for (n, t) in [([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]), ([s, 0.0, -s], [0.0, 1.0, 0.0, -1.0]), ([0.0, -1.0, 0.0], [s, 0.0, s, 1.0])] {
            let (dn, dt) = unpack_frame(pack_frame(n, t));
            assert!(n.iter().zip(dn).all(|(a, b)| (a - b).abs() < 0.01), "{n:?} -> {dn:?}");
            assert!(t.iter().zip(dt).all(|(a, b)| (a - b).abs() < 0.01), "{t:?} -> {dt:?}");
        }
    }
}
