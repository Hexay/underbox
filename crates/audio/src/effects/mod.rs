//! Native effects managed applies per voice or mixer: Steam Audio's direct, binaural and reverb
//! (our stand-ins), and the `CAudioProcessor` delay and pitch shifter. Plus the filters they share.

pub mod binaural;
pub mod direct;
pub mod processor;
pub mod reverb;

use crate::{BLOCK, Block, RATE};

/// The average of `input`'s channels (managed's `MixBuffer.MixFrom(MultiChannelBuffer)`).
pub fn downmix(input: &[Block]) -> Block {
    let mut out = crate::SILENT;
    let scale = 1.0 / input.len().max(1) as f32;
    for b in input {
        out.iter_mut().zip(b).for_each(|(o, s)| *o += s * scale);
    }
    out
}

/// A gain stepping linearly from `from` to `to` across the block.
pub fn ramp(from: f32, to: f32, i: usize) -> f32 {
    from + (to - from) * i as f32 / BLOCK as f32
}

/// One-pole low-pass at `cutoff` Hz.
#[derive(Debug, Clone, Default)]
pub struct OnePole {
    a: f32,
    y: f32,
}

impl OnePole {
    pub fn new(cutoff: f32) -> Self {
        let a = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / RATE as f32).exp();
        Self { a: a.clamp(0.0, 1.0), y: 0.0 }
    }

    /// A smoothing filter with coefficient `a` (1 passes the input through).
    pub fn with_coefficient(a: f32) -> Self {
        Self { a: a.clamp(0.0, 1.0), y: 0.0 }
    }

    pub fn set_coefficient(&mut self, a: f32) {
        self.a = a.clamp(0.0, 1.0);
    }

    pub fn tick(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
}

/// RBJ cookbook biquad.
#[derive(Debug, Clone)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    pub fn new(kind: FilterKind, cutoff: f32, q: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * cutoff.clamp(10.0, RATE as f32 * 0.49) / RATE as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let b = match kind {
            FilterKind::LowPass => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
            FilterKind::HighPass => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
            FilterKind::BandPass => [alpha, 0.0, -alpha],
        };
        let a0 = 1.0 + alpha;
        Self { b: b.map(|v| v / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }

    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A circular delay line read at whole or fractional delays.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    write: usize,
}

impl DelayLine {
    pub fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(2)], write: 0 }
    }

    /// The sample written `delay` samples ago (1 is the last one written).
    pub fn read(&self, delay: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.write + n - delay.clamp(1, n)) % n]
    }

    pub fn read_frac(&self, delay: f32) -> f32 {
        let d = delay.clamp(1.0, (self.buf.len() - 1) as f32);
        let i = d as usize;
        let t = d - i as f32;
        self.read(i) + (self.read(i + 1) - self.read(i)) * t
    }

    pub fn push(&mut self, x: f32) {
        self.buf[self.write] = x;
        self.write = (self.write + 1) % self.buf.len();
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_of_sine(f: &mut impl FnMut(f32) -> f32, freq: f32) -> f32 {
        let n = RATE as usize / 4;
        let out: Vec<f32> = (0..n).map(|i| f((2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin())).collect();
        (out[n / 2..].iter().map(|v| v * v).sum::<f32>() / (n / 2) as f32).sqrt()
    }

    #[test]
    fn biquads_pass_and_stop_where_they_should() {
        let full = std::f32::consts::FRAC_1_SQRT_2;
        let lp = |freq| rms_of_sine(&mut { let mut f = Biquad::new(FilterKind::LowPass, 1000.0, 0.707); move |x| f.tick(x) }, freq);
        let hp = |freq| rms_of_sine(&mut { let mut f = Biquad::new(FilterKind::HighPass, 1000.0, 0.707); move |x| f.tick(x) }, freq);
        assert!((lp(100.0) - full).abs() < 0.02 && lp(10000.0) < 0.02);
        assert!((hp(10000.0) - full).abs() < 0.02 && hp(100.0) < 0.02);
    }

    #[test]
    fn delay_line_reads_back_in_order() {
        let mut d = DelayLine::new(8);
        (1..=5).for_each(|v| d.push(v as f32));
        assert_eq!((d.read(1), d.read(3)), (5.0, 3.0));
        assert_eq!(d.read_frac(1.5), 4.5);
    }

    #[test]
    fn downmix_averages() {
        assert_eq!(downmix(&[[1.0; BLOCK], [0.0; BLOCK]])[7], 0.5);
    }
}
