//! BC1 (DXT1) colour and BC4 (one-channel, also BC3's alpha and BC5's halves) block encoders.
//! Range fit: endpoints from the block's bounding box, then the nearest palette entry per texel.

pub type Block = [[u8; 4]; 16];

/// Encode `w`x`h` texels in 4x4 blocks, row by row; partial edge blocks repeat their last texel.
pub fn blocks(texels: &[[u8; 4]], w: usize, h: usize, encode: impl Fn(&Block) -> Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            let block: Block = std::array::from_fn(|i| {
                let (x, y) = ((bx + i % 4).min(w - 1), (by + i / 4).min(h - 1));
                texels[x + y * w]
            });
            out.extend(encode(&block));
        }
    }
    out
}

fn to_565(c: [f32; 3]) -> u16 {
    let q = |v: f32, max: f32| (v.clamp(0.0, 255.0) * max / 255.0).round() as u16;
    (q(c[0], 31.0) << 11) | (q(c[1], 63.0) << 5) | q(c[2], 31.0)
}

fn from_565(c: u16) -> [f32; 3] {
    let e = |v: u16, max: f32| f32::from(v) * 255.0 / max;
    [e(c >> 11, 31.0), e((c >> 5) & 63, 63.0), e(c & 31, 31.0)]
}

/// Opaque four-colour BC1.
pub fn bc1(block: &Block) -> [u8; 8] {
    let rgb = block.map(|t| [f32::from(t[0]), f32::from(t[1]), f32::from(t[2])]);
    let (mut lo, mut hi) = ([255.0f32; 3], [0.0f32; 3]);
    for c in &rgb {
        (0..3).for_each(|i| (lo[i], hi[i]) = (lo[i].min(c[i]), hi[i].max(c[i])));
    }
    // Follow the colours' main diagonal: a channel falling as red (or green) rises swaps ends.
    let mean: [f32; 3] = std::array::from_fn(|i| rgb.iter().map(|c| c[i]).sum::<f32>() / 16.0);
    let cov = |a: usize, b: usize| rgb.iter().map(|c| (c[a] - mean[a]) * (c[b] - mean[b])).sum::<f32>();
    let main = if hi[0] - lo[0] >= hi[1] - lo[1] { 0 } else { 1 };
    for i in (0..3).filter(|&i| i != main) {
        if cov(main, i) < 0.0 {
            std::mem::swap(&mut lo[i], &mut hi[i]);
        }
    }
    let (mut c0, mut c1) = (to_565(hi), to_565(lo));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut out = [0u8; 8];
    out[..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    if c0 == c1 {
        return out;
    }
    let (p0, p1) = (from_565(c0), from_565(c1));
    let mix = |a: f32, b: f32| -> [f32; 3] { std::array::from_fn(|i| p0[i] * a + p1[i] * b) };
    let palette = [p0, p1, mix(2.0 / 3.0, 1.0 / 3.0), mix(1.0 / 3.0, 2.0 / 3.0)];
    let mut bits = 0u32;
    for (i, c) in rgb.iter().enumerate() {
        bits |= (nearest(&palette, |p| (0..3).map(|k| (p[k] - c[k]).powi(2)).sum()) as u32) << (2 * i);
    }
    out[4..].copy_from_slice(&bits.to_le_bytes());
    out
}

fn nearest<T>(palette: &[T], distance: impl Fn(&T) -> f32) -> usize {
    (0..palette.len()).min_by(|&a, &b| distance(&palette[a]).total_cmp(&distance(&palette[b]))).unwrap_or(0)
}

/// Eight-value BC4: two endpoints and six interpolants, 3-bit indices.
pub fn bc4(values: &[u8; 16]) -> [u8; 8] {
    let (a0, a1) = (*values.iter().max().unwrap(), *values.iter().min().unwrap());
    let mut out = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 == a1 {
        return out;
    }
    let (f0, f1) = (f32::from(a0), f32::from(a1));
    let palette: [f32; 8] = std::array::from_fn(|i| match i {
        0 => f0,
        1 => f1,
        i => (f0 * (8 - i) as f32 + f1 * (i - 1) as f32) / 7.0,
    });
    let mut bits = 0u64;
    for (i, &v) in values.iter().enumerate() {
        bits |= (nearest(&palette, |p| (p - f32::from(v)).abs()) as u64) << (3 * i);
    }
    out[2..].copy_from_slice(&bits.to_le_bytes()[..6]);
    out
}

#[cfg(test)]
pub fn decode_bc1(b: &[u8]) -> [[u8; 3]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (p0, p1) = (from_565(c0), from_565(c1));
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| {
        let w = [(1.0, 0.0), (0.0, 1.0), (2.0 / 3.0, 1.0 / 3.0), (1.0 / 3.0, 2.0 / 3.0)][((bits >> (2 * i)) & 3) as usize];
        std::array::from_fn(|k| (p0[k] * w.0 + p1[k] * w.1).round() as u8)
    })
}

#[cfg(test)]
pub fn decode_bc4(b: &[u8]) -> [u8; 16] {
    let (f0, f1) = (f32::from(b[0]), f32::from(b[1]));
    let mut raw = [0u8; 8];
    raw[..6].copy_from_slice(&b[2..8]);
    let bits = u64::from_le_bytes(raw);
    std::array::from_fn(|i| match (bits >> (3 * i)) & 7 {
        0 => b[0],
        1 => b[1],
        k => ((f0 * (8 - k) as f32 + f1 * (k - 1) as f32) / 7.0).round() as u8,
    })
}
