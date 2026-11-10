//! Separable resampling: bilinear when enlarging, an area average (box) when shrinking.

use super::FloatBitmap;

/// Source taps and weights for each destination texel along one axis of `src` texels.
fn taps(src: usize, dst: usize, clamp: bool) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f32 / dst as f32;
    let wrap = |s: i64| if clamp { s.clamp(0, src as i64 - 1) } else { s.rem_euclid(src as i64) } as usize;
    (0..dst)
        .map(|i| {
            let mut taps: Vec<(usize, f32)> = if scale > 1.0 {
                // How much of each source texel [s, s+1] lies in this texel's footprint.
                let (a, b) = (i as f32 * scale, (i + 1) as f32 * scale);
                (a.floor() as i64..b.ceil() as i64).map(|s| (wrap(s), b.min(s as f32 + 1.0) - a.max(s as f32))).collect()
            } else {
                let center = (i as f32 + 0.5) * scale - 0.5;
                let s = center.floor();
                let t = center - s;
                vec![(wrap(s as i64), 1.0 - t), (wrap(s as i64 + 1), t)]
            };
            taps.retain(|t| t.1 > 0.0);
            let total: f32 = taps.iter().map(|t| t.1).sum();
            taps.iter_mut().for_each(|t| t.1 /= total);
            taps
        })
        .collect()
}

pub fn resize(src: &FloatBitmap, w: usize, h: usize, clamp: bool) -> FloatBitmap {
    if src.width == 0 || src.height == 0 || w == 0 || h == 0 {
        return FloatBitmap::new(w, h, src.depth.max(1));
    }
    let (tx, ty) = (taps(src.width, w, clamp), taps(src.height, h, clamp));
    let mut out = FloatBitmap::new(w, h, src.depth);
    let mut row = vec![[0.0f32; 4]; w * src.height];
    for z in 0..src.depth {
        for y in 0..src.height {
            for (x, taps) in tx.iter().enumerate() {
                row[x + y * w] = weighted(taps.iter().map(|&(s, wt)| (src.texel(s, y, z), wt)));
            }
        }
        for (y, taps) in ty.iter().enumerate() {
            for x in 0..w {
                let i = out.index(x, y, z);
                out.texels[i] = weighted(taps.iter().map(|&(s, wt)| (row[x + s * w], wt)));
            }
        }
    }
    out
}

fn weighted(samples: impl Iterator<Item = ([f32; 4], f32)>) -> [f32; 4] {
    let mut acc = [0.0; 4];
    for (t, w) in samples {
        (0..4).for_each(|c| acc[c] += t[c] * w);
    }
    acc
}
