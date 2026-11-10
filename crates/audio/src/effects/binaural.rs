//! `CBinauralEffect`: places a voice around the listener. This is managed's own pan law, the one
//! `BinauralEffect.Apply` falls back to without Steam Audio, so both paths sound the same.
//! TODO: HRTF (Steam Audio's binaural effect).

use crate::Block;

/// `snd_dir_power`'s default.
pub const DIR_POWER: f32 = 0.8;
/// How far the per-ear gain moves toward its target each block.
const SMOOTHING: f32 = 0.1;

#[derive(Debug, Default, Clone)]
pub struct Binaural {
    gains: [f32; 2],
}

/// s&box's axes: +x forward, +y left.
fn ear_gain(side: f32, fwd: f32, blend: f32) -> f32 {
    let gain = (side.clamp(0.0, 1.0) + fwd).powf(DIR_POWER);
    (gain * blend).clamp(0.0001, 1.0)
}

impl Binaural {
    /// `direction` is the source relative to the listener, in listener space.
    pub fn apply(&mut self, direction: [f32; 3], spatial_blend: f32, input: &[Block], output: &mut [Block]) {
        output.iter_mut().for_each(|b| *b = crate::SILENT);
        if input.is_empty() {
            return;
        }
        let upmix = |output: &mut [Block], scale: f32| {
            for (i, o) in output.iter_mut().enumerate() {
                o.iter_mut().zip(&input[i % input.len()]).for_each(|(o, s)| *o = s * scale);
            }
        };
        if spatial_blend <= 0.0 {
            upmix(output, 1.0);
            return;
        }
        if spatial_blend < 1.0 {
            upmix(output, 1.0 - spatial_blend);
        }

        let len = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        let [x, y, _] = if len > 0.0 { direction.map(|v| v / len) } else { direction };
        let fwd = if x < 0.0 { x * -0.5 } else { x };
        let mono = super::downmix(input);
        for (ear, side) in [y, -y].into_iter().enumerate() {
            let target = ear_gain(side, fwd, spatial_blend);
            self.gains[ear] += (target - self.gains[ear]) * SMOOTHING;
            if let Some(out) = output.get_mut(ear) {
                out.iter_mut().zip(&mono).for_each(|(o, s)| *o += s * self.gains[ear]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLOCK, blocks};

    fn run(b: &mut Binaural, dir: [f32; 3], blend: f32, input: &[Block]) -> Vec<Block> {
        let mut out = blocks(2);
        b.apply(dir, blend, input, &mut out);
        out
    }

    #[test]
    fn no_spatial_blend_is_a_plain_upmix() {
        let out = run(&mut Binaural::default(), [0.0, 1.0, 0.0], 0.0, &[[0.5; BLOCK]]);
        assert_eq!((out[0][0], out[1][0]), (0.5, 0.5));
    }

    #[test]
    fn a_source_on_the_left_is_louder_left_and_settles_on_the_pan_law() {
        let mut b = Binaural::default();
        let input = [[1.0; BLOCK]];
        let first = run(&mut b, [0.0, 2.0, 0.0], 1.0, &input);
        assert!((first[0][0] - 0.1).abs() < 1e-6, "first block moves 0.1 of the way to 1.0");
        assert!((first[1][0] - 0.00001).abs() < 1e-6);
        let mut last = first;
        for _ in 0..200 {
            last = run(&mut b, [0.0, 2.0, 0.0], 1.0, &input);
        }
        assert!((last[0][0] - 1.0).abs() < 1e-4 && last[1][0] < 0.001);
    }

    #[test]
    fn straight_ahead_is_centred_and_behind_is_quieter() {
        let mut ahead = Binaural::default();
        let mut behind = Binaural::default();
        let input = [[1.0; BLOCK]];
        let (mut a, mut z) = (blocks(2), blocks(2));
        for _ in 0..200 {
            a = run(&mut ahead, [1.0, 0.0, 0.0], 1.0, &input);
            z = run(&mut behind, [-1.0, 0.0, 0.0], 1.0, &input);
        }
        assert!((a[0][0] - a[1][0]).abs() < 1e-6 && (a[0][0] - 1.0).abs() < 1e-4);
        assert!((z[0][0] - 0.5f32.powf(DIR_POWER)).abs() < 1e-4, "behind is half as forward, then the power curve");
    }

    #[test]
    fn half_blend_keeps_half_the_dry_upmix() {
        let mut b = Binaural::default();
        let out = run(&mut b, [1.0, 0.0, 0.0], 0.5, &[[1.0; BLOCK]]);
        let wet = 0.1 * (1.0f32.powf(DIR_POWER) * 0.5);
        assert!((out[0][0] - (0.5 + wet)).abs() < 1e-6);
    }
}
