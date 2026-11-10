//! Our stand-in for OVRLipSync (closed Oculus SDK): viseme weights from loudness and the
//! zero-crossing rate. Quiet is `sil`; voiced, low-ZCR audio opens toward `aa`/`oh`; noisy,
//! high-ZCR audio reads as the fricatives `SS`/`FF`. TODO: formant-based vowel choice.

/// `OVRLipSync.Viseme` order.
pub const VISEME_COUNT: usize = 15;
const SIL: usize = 0;
const FF: usize = 2;
const SS: usize = 7;
const AA: usize = 10;
const OH: usize = 13;

/// RMS below this is silence.
const GATE: f32 = 0.01;
/// How far each frame's weights move toward the new estimate.
const SMOOTHING: f32 = 0.5;

pub struct Context {
    pub rate: u32,
    pub frame: i32,
    weights: [f32; VISEME_COUNT],
}

impl Context {
    pub fn new(rate: u32) -> Self {
        let mut weights = [0.0; VISEME_COUNT];
        weights[SIL] = 1.0;
        Self { rate: rate.max(1), frame: 0, weights }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.rate);
    }

    fn estimate(samples: &[f32], rate: u32) -> [f32; VISEME_COUNT] {
        let mut w = [0.0; VISEME_COUNT];
        let n = samples.len().max(1) as f32;
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / n).sqrt();
        if rms < GATE {
            w[SIL] = 1.0;
            return w;
        }
        let crossings = samples.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count() as f32;
        let zcr_hz = crossings / 2.0 * rate as f32 / n;
        let open = (rms * 4.0).min(1.0);
        let noisy = ((zcr_hz - 1500.0) / 2500.0).clamp(0.0, 1.0);
        w[SIL] = 1.0 - open;
        w[SS] = open * noisy * 0.7;
        w[FF] = open * noisy * 0.3;
        w[AA] = open * (1.0 - noisy) * 0.7;
        w[OH] = open * (1.0 - noisy) * 0.3;
        w
    }

    /// One frame of mono samples; the smoothed weights (summing to 1).
    pub fn process(&mut self, samples: &[f32]) -> [f32; VISEME_COUNT] {
        let target = Self::estimate(samples, self.rate);
        self.weights.iter_mut().zip(target).for_each(|(w, t)| *w += (t - *w) * SMOOTHING);
        self.frame += 1;
        self.weights
    }

    /// Milliseconds of audio one frame of `samples` covers (OVR's reported frame delay).
    pub fn delay_ms(&self, samples: usize) -> i32 {
        (samples as u64 * 1000 / self.rate as u64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, amp: f32) -> Vec<f32> {
        (0..512).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 48000.0).sin()).collect()
    }

    fn settle(ctx: &mut Context, s: &[f32]) -> [f32; VISEME_COUNT] {
        (0..20).map(|_| ctx.process(s)).last().unwrap()
    }

    #[test]
    fn silence_is_sil_and_weights_sum_to_one() {
        let mut ctx = Context::new(48000);
        let w = settle(&mut ctx, &[0.0; 512]);
        assert!((w[SIL] - 1.0).abs() < 1e-5);
        let w = settle(&mut ctx, &tone(200.0, 0.5));
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert_eq!(ctx.frame, 40);
    }

    #[test]
    fn loud_low_tones_open_the_mouth_and_hiss_reads_as_fricatives() {
        let mut ctx = Context::new(48000);
        let vowel = settle(&mut ctx, &tone(200.0, 0.5));
        assert!(vowel[AA] > 0.5 && vowel[SS] < 0.01);
        ctx.reset();
        let hiss = settle(&mut ctx, &tone(6000.0, 0.5));
        assert!(hiss[SS] > 0.5 && hiss[AA] < 0.01);
        assert_eq!(ctx.delay_ms(480), 10);
    }
}
