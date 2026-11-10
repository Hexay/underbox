//! `ImageFormat` texel layouts (Sandbox.Engine's ImageFormat.cs values): decoding raw texels into
//! float RGBA and encoding back, including BC1/BC3/BC4/BC5 block compression.

use super::{FloatBitmap, Gamma, bc};
use half::f16;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Unorm8,
    Unorm16,
    Half,
    Float,
}

/// Where each stored component goes: a channel, luminance (into r, g and b), or padding.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Chan {
    R,
    G,
    B,
    A,
    L,
    X,
}

use Chan::*;

fn layout(format: i64) -> Option<(Kind, &'static [Chan])> {
    use Kind::*;
    Some(match format {
        0 | 42 | 81 => (Unorm8, &[R, G, B, A]),
        1 | 43 => (Unorm8, &[A, B, G, R]),
        2 | 46 => (Unorm8, &[R, G, B]),
        3 | 47 => (Unorm8, &[B, G, R]),
        5 | 49 => (Unorm8, &[L]),
        6 => (Unorm8, &[L, A]),
        8 => (Unorm8, &[A]),
        11 | 44 => (Unorm8, &[A, R, G, B]),
        12 | 45 | 52 | 82 => (Unorm8, &[B, G, R, A]),
        16 | 41 | 51 | 83 => (Unorm8, &[B, G, R, X]),
        55 => (Unorm8, &[R, G, B, X]),
        25 | 50 => (Unorm16, &[R, G, B, A]),
        79 => (Unorm16, &[R, G]),
        80 => (Unorm16, &[R]),
        24 => (Half, &[R, G, B, A]),
        53 => (Half, &[R, G]),
        59 => (Half, &[R]),
        94 => (Half, &[L]),
        27 => (Float, &[R]),
        28 => (Float, &[R, G, B]),
        29 => (Float, &[R, G, B, A]),
        54 => (Float, &[R, G]),
        _ => return None,
    })
}

fn size(kind: Kind) -> usize {
    match kind {
        Kind::Unorm8 => 1,
        Kind::Unorm16 | Kind::Half => 2,
        Kind::Float => 4,
    }
}

const DXT1: i64 = 13;
const DXT5: i64 = 15;
const ATI2N: i64 = 36;
const ATI1N: i64 = 37;

/// Bytes `w`x`h` texels take, if we can encode the format.
pub fn byte_size(format: i64, w: usize, h: usize) -> Option<usize> {
    let blocks = w.div_ceil(4) * h.div_ceil(4);
    match format {
        DXT1 | ATI1N => Some(blocks * 8),
        DXT5 | ATI2N => Some(blocks * 16),
        _ => layout(format).map(|(k, chans)| w * h * size(k) * chans.len()),
    }
}

/// Raw texels in `format` to linear float RGBA; `gamma` is the source's (applied to colour, not alpha).
pub fn decode(bytes: &[u8], w: usize, h: usize, format: i64, gamma: Gamma) -> Option<FloatBitmap> {
    let (kind, chans) = layout(format)?;
    let step = size(kind);
    let stride = step * chans.len();
    let bytes = bytes.get(..w * h * stride)?;
    let texels = bytes
        .chunks_exact(stride)
        .map(|px| {
            let mut t = [0.0, 0.0, 0.0, 1.0];
            for (chan, v) in chans.iter().zip(px.chunks_exact(step)) {
                let v = read(kind, v);
                let colour = |v| if kind == Kind::Unorm8 || kind == Kind::Unorm16 { gamma.to_linear(v) } else { v };
                match chan {
                    R => t[0] = colour(v),
                    G => t[1] = colour(v),
                    B => t[2] = colour(v),
                    A => t[3] = v,
                    L => t[..3].fill(colour(v)),
                    X => {}
                }
            }
            t
        })
        .collect();
    Some(FloatBitmap::from_texels(w, h, texels))
}

fn read(kind: Kind, b: &[u8]) -> f32 {
    match kind {
        Kind::Unorm8 => f32::from(b[0]) / 255.0,
        Kind::Unorm16 => f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0,
        Kind::Half => f16::from_le_bytes([b[0], b[1]]).to_f32(),
        Kind::Float => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
    }
}

fn unorm8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The first slice in `format`; `srgb` stores colour sRGB-encoded (8- and 16-bit formats only).
pub fn encode(bmp: &FloatBitmap, format: i64, srgb: bool) -> Option<Vec<u8>> {
    let (w, h) = (bmp.width, bmp.height);
    let texels = &bmp.texels[..w * h];
    let gamma = if srgb { Gamma::Srgb } else { Gamma::Linear };
    let rgba8 = || -> Vec<[u8; 4]> {
        let c = |v| unorm8(gamma.from_linear(v));
        texels.iter().map(|t| [c(t[0]), c(t[1]), c(t[2]), unorm8(t[3])]).collect()
    };
    match format {
        DXT1 => return Some(bc::blocks(&rgba8(), w, h, |b| bc::bc1(b).to_vec())),
        DXT5 => return Some(bc::blocks(&rgba8(), w, h, |b| [bc::bc4(&b.map(|t| t[3])), bc::bc1(b)].concat())),
        ATI1N => return Some(bc::blocks(&rgba8(), w, h, |b| bc::bc4(&b.map(|t| t[0])).to_vec())),
        ATI2N => return Some(bc::blocks(&rgba8(), w, h, |b| [bc::bc4(&b.map(|t| t[0])), bc::bc4(&b.map(|t| t[1]))].concat())),
        _ => {}
    }
    let (kind, chans) = layout(format)?;
    let mut out = Vec::with_capacity(byte_size(format, w, h)?);
    for t in texels {
        for chan in chans {
            let colour = |v| if kind == Kind::Unorm8 || kind == Kind::Unorm16 { gamma.from_linear(v) } else { v };
            let v = match chan {
                R => colour(t[0]),
                G => colour(t[1]),
                B => colour(t[2]),
                A => t[3],
                L => colour(0.299 * t[0] + 0.587 * t[1] + 0.114 * t[2]),
                X => 1.0,
            };
            match kind {
                Kind::Unorm8 => out.push(unorm8(v)),
                Kind::Unorm16 => out.extend(((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes()),
                Kind::Half => out.extend(f16::from_f32(v).to_le_bytes()),
                Kind::Float => out.extend(v.to_le_bytes()),
            }
        }
    }
    Some(out)
}
