//! The processors a DSP preset chains, one sample at a time. Parameter layouts follow
//! `DspPreset.Add*`. TODO: delay-time modulation (mod delay, reverb `modDepth`/`modRate`) and the
//! delay types beyond plain and low-passed.

use super::kind;
use crate::RATE;
use crate::effects::{Biquad, DelayLine, FilterKind, OnePole};

fn ms(v: f32) -> usize {
    ((v.max(0.0) / 1000.0 * RATE as f32).round() as usize).max(1)
}

fn filter_kind(code: f32) -> FilterKind {
    match code as i32 {
        1 => FilterKind::HighPass,
        2 => FilterKind::BandPass,
        _ => FilterKind::LowPass,
    }
}

fn q(cutoff: f32, width: f32) -> f32 {
    if width > 0.0 { cutoff / width } else { std::f32::consts::FRAC_1_SQRT_2 }
}

pub struct Comb {
    line: DelayLine,
    delay: usize,
    damping: OnePole,
}

pub enum Unit {
    /// Echo: dry plus `gain` times the line; the line feeds back through an optional filter.
    Delay { line: DelayLine, delay: usize, feedback: f32, gain: f32, filter: Option<Biquad> },
    /// Parallel low-passed combs between the min and max delay, added to the dry signal.
    Reverb { combs: Vec<Comb>, feedback: f32, gain: f32 },
    Filter { biquad: Biquad, gain: f32 },
    /// Schroeder all-passes: smear transients without colouring the spectrum.
    Diffusor { stages: Vec<(DelayLine, usize)>, coefficient: f32, gain: f32 },
    /// Gain, soft clipping and tremolo.
    Amplifier { gain: f32, threshold: f32, mix: f32, rate: f32, depth: f32, t: u64 },
}

impl Unit {
    pub fn new(code: i32, p: &[f32]) -> Option<Self> {
        let p = |i: usize| p.get(i).copied().unwrap_or(0.0);
        Some(match code {
            kind::DELAY | kind::MOD_DELAY => {
                let delay = ms(p(1));
                let filter = (p(5) > 0.0).then(|| Biquad::new(filter_kind(p(4)), p(5), q(p(5), p(6))));
                Self::Delay { line: DelayLine::new(delay + 1), delay, feedback: p(2).clamp(0.0, 0.99), gain: p(3), filter }
            }
            kind::REVERB => {
                let count = (p(2) as usize).clamp(1, 12);
                let combs = (0..count)
                    .map(|k| {
                        let t = if count == 1 { 0.0 } else { k as f32 / (count - 1) as f32 };
                        let delay = ms(p(1) + (p(0) - p(1)) * t);
                        let damping = if p(5) > 0.0 { OnePole::new(p(5)) } else { OnePole::with_coefficient(1.0) };
                        Comb { line: DelayLine::new(delay + 1), delay, damping }
                    })
                    .collect();
                Self::Reverb { combs, feedback: p(3).clamp(0.0, 0.98), gain: p(4) }
            }
            kind::FILTER => Self::Filter { biquad: Biquad::new(filter_kind(p(0)), p(1), q(p(1), p(2))), gain: p(4) },
            kind::DIFFUSOR => {
                const BASE_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];
                let stages = BASE_MS
                    .iter()
                    .take((p(1) as usize).clamp(1, 4))
                    .map(|b| {
                        let d = ms(b * p(0).max(0.1));
                        (DelayLine::new(d + 1), d)
                    })
                    .collect();
                Self::Diffusor { stages, coefficient: p(2).clamp(0.0, 0.9), gain: if p(3) > 0.0 { p(3) } else { 1.0 } }
            }
            kind::AMPLIFIER => Self::Amplifier { gain: p(0), threshold: p(1), mix: p(2).clamp(0.0, 1.0), rate: p(4), depth: p(5).clamp(0.0, 1.0), t: 0 },
            _ => return None,
        })
    }

    pub fn tick(&mut self, x: f32) -> f32 {
        match self {
            Self::Delay { line, delay, feedback, gain, filter } => {
                let wet = line.read(*delay);
                let back = filter.as_mut().map_or(wet, |f| f.tick(wet));
                line.push(x + *feedback * back);
                x + *gain * wet
            }
            Self::Reverb { combs, feedback, gain } => {
                let n = combs.len() as f32;
                let wet: f32 = combs
                    .iter_mut()
                    .map(|c| {
                        let out = c.line.read(c.delay);
                        c.line.push(x + *feedback * c.damping.tick(out));
                        out
                    })
                    .sum();
                x + *gain * wet / n
            }
            Self::Filter { biquad, gain } => *gain * biquad.tick(x),
            Self::Diffusor { stages, coefficient, gain } => {
                let g = *coefficient;
                let y = stages.iter_mut().fold(x, |x, (line, d)| {
                    let delayed = line.read(*d);
                    let y = -g * x + delayed;
                    line.push(x + g * y);
                    y
                });
                *gain * y
            }
            Self::Amplifier { gain, threshold, mix, rate, depth, t } => {
                let clipped = if *threshold > 0.0 { (x / *threshold).tanh() * *threshold } else { x };
                let shaped = x + (clipped - x) * *mix;
                let phase = 2.0 * std::f32::consts::PI * *rate * *t as f32 / RATE as f32;
                *t += 1;
                *gain * shaped * (1.0 - *depth * 0.5 * (1.0 + phase.sin()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(u: &mut Unit, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&x| u.tick(x)).collect()
    }

    #[test]
    fn delay_echoes_and_feeds_back() {
        let mut d = Unit::new(kind::DELAY, &[0.0, 1000.0 / RATE as f32 * 3.0, 0.5, 0.8, 0.0, 0.0]).unwrap();
        let mut x = vec![0.0; 10];
        x[0] = 1.0;
        assert_eq!(run(&mut d, &x), vec![1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.4, 0.0, 0.0, 0.2]);
    }

    #[test]
    fn all_pass_diffusion_keeps_the_energy() {
        let mut d = Unit::new(kind::DIFFUSOR, &[1.0, 2.0, 0.5, 0.0]).unwrap();
        let mut x = vec![0.0; RATE as usize];
        x[0] = 1.0;
        let energy: f32 = run(&mut d, &x).iter().map(|v| v * v).sum();
        assert!((energy - 1.0).abs() < 1e-3, "{energy}");
    }

    #[test]
    fn amplifier_gain_and_clip() {
        let mut a = Unit::new(kind::AMPLIFIER, &[2.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
        let y = a.tick(10.0);
        assert!((y - 2.0 * 0.5 * (20.0f32).tanh()).abs() < 1e-6);
        let mut flat = Unit::new(kind::FILTER, &[0.0, 20000.0, 0.0, 1.0, 1.0]).unwrap();
        let ys = run(&mut flat, &[1.0; 2000]);
        assert!((ys[1999] - 1.0).abs() < 1e-3, "low-pass passes DC");
    }
}
