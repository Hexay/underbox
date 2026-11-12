//! `CBinauralEffect`: Steam Audio's binaural effect with its default HRTF, as real engine2 links it.
//! Without the HRTF, `CBinauralEffect_Create` returns null and managed uses its own pan law.

use crate::{BLOCK, Block};
use steamaudio_sys::Interpolation;

const _: () = assert!(BLOCK == steamaudio_sys::FRAME);

pub struct Binaural(steamaudio_sys::Binaural);

/// s&box's listener space (+x ahead, +y left, +z up) to Steam Audio's (+x right, +y up, -z ahead).
fn to_steam_audio([x, y, z]: [f32; 3]) -> [f32; 3] {
    if x == 0.0 && y == 0.0 && z == 0.0 { [0.0, 0.0, -1.0] } else { [-y, z, -x] }
}

impl Binaural {
    pub fn new() -> Option<Self> {
        steamaudio_sys::Binaural::new().map(Self)
    }

    /// `direction` is the source relative to the listener, in listener space. Mono and stereo go in
    /// as they are (Steam Audio's spatial blend only acts on stereo, or on mono with bilinear
    /// lookups); more channels are averaged to mono first.
    pub fn apply(&mut self, direction: [f32; 3], spatial_blend: f32, nearest: bool, input: &[Block], output: &mut [Block]) {
        output.iter_mut().for_each(|b| *b = crate::SILENT);
        if input.is_empty() || output.is_empty() {
            return;
        }
        let mut src: Vec<Block> = if input.len() <= 2 { input.to_vec() } else { vec![super::downmix(input)] };
        let interpolation = if nearest { Interpolation::Nearest } else { Interpolation::Bilinear };
        let mut ears = [crate::SILENT; 2];
        if !self.0.apply(to_steam_audio(direction), interpolation, spatial_blend, &mut src, &mut ears) {
            return;
        }
        if let [only] = output {
            only.iter_mut().zip(ears[0].iter().zip(&ears[1])).for_each(|(o, (l, r))| *o = 0.5 * (l + r));
        } else {
            output[..2].copy_from_slice(&ears);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks;

    fn rms_db(b: &Block) -> f32 {
        10.0 * (b.iter().map(|v| v * v).sum::<f32>() / BLOCK as f32).log10()
    }

    fn settle(b: &mut Binaural, dir: [f32; 3], blend: f32, nearest: bool, input: &[Block]) -> Vec<Block> {
        let mut out = blocks(2);
        for _ in 0..4 {
            b.apply(dir, blend, nearest, input, &mut out);
        }
        out
    }

    fn noise() -> Block {
        let mut x = 1u32;
        std::array::from_fn(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        })
    }

    #[test]
    fn a_source_on_the_left_is_louder_left_and_one_on_the_right_louder_right() {
        let input = [noise()];
        let left = settle(&mut Binaural::new().unwrap(), [0.0, 200.0, 0.0], 1.0, false, &input);
        let right = settle(&mut Binaural::new().unwrap(), [0.0, -200.0, 0.0], 1.0, false, &input);
        assert!(rms_db(&left[0]) > rms_db(&left[1]) + 3.0);
        assert!(rms_db(&right[1]) > rms_db(&right[0]) + 3.0);
    }

    #[test]
    fn no_spatial_blend_with_bilinear_lookups_is_the_dry_signal_in_both_ears() {
        let input = [noise()];
        let out = settle(&mut Binaural::new().unwrap(), [0.0, 1.0, 0.0], 0.0, false, &input);
        assert!((rms_db(&out[0]) - rms_db(&input[0])).abs() < 0.5 && (rms_db(&out[1]) - rms_db(&input[0])).abs() < 0.5);
    }

    #[test]
    fn extra_output_channels_stay_silent() {
        let mut out = blocks(4);
        let mut b = Binaural::new().unwrap();
        b.apply([1.0, 0.0, 0.0], 1.0, true, &[noise()], &mut out);
        assert!(out[0].iter().any(|&v| v != 0.0) && out[2..].iter().all(|c| c.iter().all(|&v| v == 0.0)));
    }
}
