//! `CAudioProcessor`: the native delay (`DelayProcessor`) and pitch shifter (`PitchProcessor`),
//! controlled by named parameters.

use super::{DelayLine, db_to_gain};
use crate::{BLOCK, Block, MAX_CHANNELS, RATE};

const MAX_DELAY_SECONDS: usize = 3;
/// The pitch shifter's grain: two taps half a window apart, crossfaded.
const WINDOW: f32 = 2048.0;

#[derive(Debug, Clone)]
pub enum Processor {
    Delay { lines: Vec<DelayLine>, delay_ms: f32, gain: f32, feedback: f32 },
    Pitch { lines: Vec<DelayLine>, ratio: f32, phase: Vec<f32> },
}

impl Processor {
    pub fn delay(channels: usize) -> Self {
        let lines = (0..channels.clamp(1, MAX_CHANNELS)).map(|_| DelayLine::new(MAX_DELAY_SECONDS * RATE as usize + 1)).collect();
        Self::Delay { lines, delay_ms: 200.0, gain: db_to_gain(-10.0), feedback: 0.0 }
    }

    pub fn pitch_shift(channels: usize) -> Self {
        let n = channels.clamp(1, MAX_CHANNELS);
        Self::Pitch { lines: (0..n).map(|_| DelayLine::new(WINDOW as usize + 2)).collect(), ratio: 1.0, phase: vec![0.0; n] }
    }

    /// `SetControlParameter`: false for a name this processor doesn't have.
    pub fn set_control(&mut self, name: &str, value: f32) -> bool {
        match (self, name) {
            (Self::Delay { delay_ms, .. }, "delay") => *delay_ms = value.clamp(0.0, MAX_DELAY_SECONDS as f32 * 1000.0),
            (Self::Delay { gain, .. }, "gain") => *gain = db_to_gain(value),
            (Self::Delay { feedback, .. }, "feedback") => *feedback = value.clamp(0.0, 0.99),
            (Self::Pitch { ratio, .. }, "pitchScale") => *ratio = value.clamp(0.25, 4.0),
            _ => return false,
        }
        true
    }

    /// The first `channels` channels of `input` into `output`.
    pub fn process(&mut self, input: &[Block], output: &mut [Block], channels: usize) {
        match self {
            Self::Delay { lines, delay_ms, gain, feedback } => {
                let d = ((*delay_ms / 1000.0 * RATE as f32) as usize).max(1);
                for ((line, x), y) in lines.iter_mut().zip(input).zip(output.iter_mut()).take(channels) {
                    for i in 0..BLOCK {
                        let wet = line.read(d);
                        y[i] = x[i] + *gain * wet;
                        line.push(x[i] + *feedback * wet);
                    }
                }
            }
            Self::Pitch { lines, ratio, phase } => {
                for (((line, phase), x), y) in lines.iter_mut().zip(phase.iter_mut()).zip(input).zip(output.iter_mut()).take(channels) {
                    for i in 0..BLOCK {
                        line.push(x[i]);
                        if *ratio == 1.0 {
                            y[i] = x[i];
                            continue;
                        }
                        *phase = (*phase + 1.0 - *ratio).rem_euclid(WINDOW);
                        let other = (*phase + WINDOW / 2.0) % WINDOW;
                        let weight = |p: f32| 1.0 - (p - WINDOW / 2.0).abs() / (WINDOW / 2.0);
                        y[i] = weight(*phase) * line.read_frac(*phase + 1.0) + weight(other) * line.read_frac(other + 1.0);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks;

    fn impulse() -> Vec<Block> {
        let mut b = crate::SILENT;
        b[0] = 1.0;
        vec![b]
    }

    #[test]
    fn delay_echoes_at_the_delay_with_its_gain() {
        let mut p = Processor::delay(1);
        assert!(p.set_control("delay", 5.0) && p.set_control("gain", -6.0));
        assert!(!p.set_control("pitchScale", 2.0));
        let mut out = blocks(1);
        p.process(&impulse(), &mut out, 1);
        let at = (0.005 * RATE as f32) as usize;
        assert_eq!(out[0][0], 1.0);
        assert!((out[0][at] - db_to_gain(-6.0)).abs() < 1e-6);
        assert_eq!(out[0].iter().filter(|&&v| v != 0.0).count(), 2);
    }

    #[test]
    fn pitch_scale_one_passes_through_and_two_doubles_the_frequency() {
        let sine = |n: usize, f: f32| (2.0 * std::f32::consts::PI * f * n as f32 / RATE as f32).sin();
        let crossings = |ratio: f32| {
            let mut p = Processor::pitch_shift(1);
            assert!(p.set_control("pitchScale", ratio));
            let mut out = blocks(1);
            let mut ys = Vec::new();
            for b in 0..40 {
                let input = vec![std::array::from_fn(|i| sine(b * BLOCK + i, 200.0))];
                p.process(&input, &mut out, 1);
                if ratio == 1.0 {
                    assert_eq!(out[0], input[0]);
                }
                ys.extend_from_slice(&out[0]);
            }
            ys[BLOCK * 8..].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32
        };
        let base = crossings(1.0);
        let doubled = crossings(2.0);
        assert!((doubled / base - 2.0).abs() < 0.15, "{base} -> {doubled} upward crossings");
    }
}
