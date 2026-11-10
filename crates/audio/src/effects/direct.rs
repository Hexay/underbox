//! `CDirectEffect`: Steam Audio's direct-path effect. Distance attenuation, air absorption and
//! occlusion with transmission, as one gain per band over Steam Audio's three bands, ramped
//! across the block so gain changes don't click.

use super::{OnePole, ramp};
use crate::{BLOCK, Block};

/// Steam Audio's band edges (Hz): low < 800 <= mid < 8000 <= high.
const LOW_MID: f32 = 800.0;
const MID_HIGH: f32 = 8000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectParams {
    pub distance: f32,
    pub air: [f32; 3],
    /// 1 is unoccluded.
    pub occlusion: f32,
    pub transmission: [f32; 3],
}

impl DirectParams {
    pub fn band_gains(&self) -> [f32; 3] {
        std::array::from_fn(|b| self.distance * self.air[b] * (self.occlusion + (1.0 - self.occlusion) * self.transmission[b]))
    }
}

#[derive(Debug, Clone)]
struct Split {
    low: OnePole,
    low_mid: OnePole,
}

impl Default for Split {
    fn default() -> Self {
        Self { low: OnePole::new(LOW_MID), low_mid: OnePole::new(MID_HIGH) }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Direct {
    gains: Option<[f32; 3]>,
    splits: Vec<Split>,
}

impl Direct {
    pub fn apply(&mut self, p: &DirectParams, input: &[Block], output: &mut [Block]) {
        let to = p.band_gains();
        let from = self.gains.replace(to).unwrap_or(to);
        self.splits.resize_with(input.len(), Split::default);
        for (ch, out) in output.iter_mut().enumerate() {
            let (Some(x), Some(split)) = (input.get(ch), self.splits.get_mut(ch)) else {
                *out = crate::SILENT;
                continue;
            };
            for i in 0..BLOCK {
                let low = split.low.tick(x[i]);
                let low_mid = split.low_mid.tick(x[i]);
                let bands = [low, low_mid - low, x[i] - low_mid];
                out[i] = (0..3).map(|b| bands[b] * ramp(from[b], to[b], i)).sum();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks;

    fn params(distance: f32, occlusion: f32, transmission: [f32; 3]) -> DirectParams {
        DirectParams { distance, air: [1.0; 3], occlusion, transmission }
    }

    #[test]
    fn band_gains_follow_steam_audio() {
        let p = DirectParams { distance: 0.5, air: [1.0, 0.8, 0.5], occlusion: 0.25, transmission: [0.4, 0.2, 0.0] };
        let g = p.band_gains();
        assert!((g[0] - 0.5 * (0.25 + 0.75 * 0.4)).abs() < 1e-6);
        assert!((g[1] - 0.4 * (0.25 + 0.75 * 0.2)).abs() < 1e-6);
        assert!((g[2] - 0.25 * 0.25).abs() < 1e-6);
    }

    #[test]
    fn equal_band_gains_scale_the_input_exactly() {
        let mut d = Direct::default();
        let input: Vec<Block> = vec![std::array::from_fn(|i| ((i * 7919) % 200) as f32 / 100.0 - 1.0)];
        let mut out = blocks(1);
        d.apply(&params(0.5, 1.0, [0.0; 3]), &input, &mut out);
        out[0].iter().zip(&input[0]).for_each(|(o, x)| assert!((o - 0.5 * x).abs() < 1e-5));
    }

    #[test]
    fn gain_changes_ramp_across_the_next_block() {
        let mut d = Direct::default();
        let input = vec![[1.0; BLOCK]];
        let mut out = blocks(1);
        d.apply(&params(1.0, 1.0, [0.0; 3]), &input, &mut out);
        d.apply(&params(0.0, 1.0, [0.0; 3]), &input, &mut out);
        assert!((out[0][0] - 1.0).abs() < 1e-5 && (out[0][BLOCK / 2] - 0.5).abs() < 1e-5);
        assert!(out[0][BLOCK - 1] < 0.01);
    }

    #[test]
    fn full_occlusion_with_low_transmission_muffles_highs() {
        let sine = |f: f32| -> Vec<Block> {
            vec![std::array::from_fn(|i| (2.0 * std::f32::consts::PI * f * i as f32 / crate::RATE as f32).sin())]
        };
        let level = |freq: f32| {
            let mut d = Direct::default();
            let mut out = blocks(1);
            for _ in 0..4 {
                d.apply(&params(1.0, 0.0, [1.0, 0.1, 0.0]), &sine(freq), &mut out);
            }
            out[0].iter().map(|v| v * v).sum::<f32>()
        };
        assert!(level(100.0) > 20.0 * level(12000.0));
    }
}
