//! `CReverbEffect`: a mixer's shared reverb from three-band decay times. A four-line feedback
//! delay network; each line's gain makes the mid band decay by 60 dB in `t60_mid`, and a damping
//! filter in the loop shortens the highs toward `t60_high`. TODO: the low band, Steam Audio's
//! parametric reverb proper.

use super::{DelayLine, OnePole};
use crate::{BLOCK, Block, RATE};

/// Mutually prime line lengths, 23 to 41 ms.
const LENGTHS: [usize; 4] = [1031, 1327, 1523, 1801];

#[derive(Debug, Clone)]
pub struct Reverb {
    lines: Vec<DelayLine>,
    damping: Vec<OnePole>,
}

impl Default for Reverb {
    fn default() -> Self {
        Self { lines: LENGTHS.iter().map(|&n| DelayLine::new(n + 1)).collect(), damping: vec![OnePole::with_coefficient(1.0); 4] }
    }
}

/// The per-pass gain that decays by 60 dB over `t60` seconds for a loop of `len` samples.
pub fn line_gain(len: usize, t60: f32) -> f32 {
    if t60 <= 0.0 { 0.0 } else { 10f32.powf(-3.0 * len as f32 / (t60 * RATE as f32)) }
}

impl Reverb {
    /// Mono in (the channels averaged), the same tail in every output channel.
    pub fn apply(&mut self, _t60_low: f32, t60_mid: f32, t60_high: f32, input: &[Block], output: &mut [Block]) {
        let x = super::downmix(input);
        let gains = LENGTHS.map(|n| line_gain(n, t60_mid));
        let damp = if t60_mid > 0.0 { (t60_high / t60_mid).clamp(0.05, 1.0) } else { 1.0 };
        self.damping.iter_mut().for_each(|d| d.set_coefficient(damp));
        let mut wet = crate::SILENT;
        for i in 0..BLOCK {
            let o: [f32; 4] = std::array::from_fn(|l| self.lines[l].read(LENGTHS[l]));
            // Householder-style mix: Hadamard 4x4, scaled to stay energy-preserving.
            let h = [o[0] + o[1] + o[2] + o[3], o[0] - o[1] + o[2] - o[3], o[0] + o[1] - o[2] - o[3], o[0] - o[1] - o[2] + o[3]];
            for l in 0..4 {
                let fb = self.damping[l].tick(h[l] * 0.5) * gains[l];
                self.lines[l].push(x[i] + fb);
            }
            wet[i] = (o[0] + o[1] + o[2] + o[3]) * 0.25;
        }
        output.iter_mut().for_each(|b| *b = wet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks;

    fn impulse_response(t60_mid: f32, t60_high: f32, blocks_n: usize) -> Vec<f32> {
        let mut r = Reverb::default();
        let mut out = blocks(1);
        let mut first = crate::SILENT;
        first[0] = 1.0;
        let mut ir = Vec::new();
        for n in 0..blocks_n {
            r.apply(t60_mid, t60_mid, t60_high, &[if n == 0 { first } else { crate::SILENT }], &mut out);
            ir.extend_from_slice(&out[0]);
        }
        ir
    }

    fn energy(s: &[f32]) -> f32 {
        s.iter().map(|v| v * v).sum()
    }

    #[test]
    fn silence_in_is_silence_out_and_the_tail_starts_after_the_shortest_line() {
        let ir = impulse_response(1.0, 1.0, 4);
        assert!(ir[..LENGTHS[0]].iter().all(|&v| v == 0.0));
        assert_eq!(ir[LENGTHS[0]], 0.25);
    }

    #[test]
    fn the_tail_decays_60_db_in_t60() {
        let t60 = 0.5;
        let ir = impulse_response(t60, t60, 130);
        let window = 2048;
        let at = |s: f32| energy(&ir[(s * RATE as f32) as usize..][..window]);
        let drop_db = 10.0 * (at(0.1) / at(0.1 + t60)).log10();
        assert!((50.0..70.0).contains(&drop_db), "{drop_db} dB over T60");
    }

    #[test]
    fn longer_t60_and_brighter_highs_ring_longer() {
        let tail = |mid, high| energy(&impulse_response(mid, high, 90)[RATE as usize / 2..]);
        assert!(tail(2.0, 2.0) > 10.0 * tail(0.5, 0.5));
        assert!(tail(2.0, 2.0) > tail(2.0, 0.3));
        assert_eq!(line_gain(100, 0.0), 0.0);
    }
}
