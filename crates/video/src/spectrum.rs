//! What managed's visualizers read: the last [`WINDOW`] mono samples written to the mixer, as
//! 512 FFT magnitudes (`GetSpectrum`) and an RMS level (`GetAmplitude`).

use std::collections::VecDeque;
use std::f32::consts::PI;

pub const WINDOW: usize = 1024;

#[derive(Default)]
pub struct Recent(VecDeque<f32>);

impl Recent {
    pub fn push(&mut self, interleaved: &[i16], channels: usize) {
        let channels = channels.max(1);
        for f in interleaved.chunks_exact(channels) {
            self.0.push_back(f.iter().map(|s| *s as f32).sum::<f32>() / (channels as f32 * 32768.0));
        }
        let extra = self.0.len().saturating_sub(WINDOW);
        self.0.drain(..extra);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn amplitude(&self) -> f32 {
        if self.0.is_empty() {
            return 0.0;
        }
        (self.0.iter().map(|s| s * s).sum::<f32>() / self.0.len() as f32).sqrt()
    }

    /// `WINDOW / 2` magnitudes, Hann-windowed, normalised so a full-scale sine peaks near 1.
    /// Empty until a window's worth has been written.
    pub fn spectrum(&self) -> Vec<f32> {
        if self.0.len() < WINDOW {
            return Vec::new();
        }
        let hann = |i: usize| 0.5 - 0.5 * (2.0 * PI * i as f32 / WINDOW as f32).cos();
        let mut re: Vec<f32> = self.0.iter().enumerate().map(|(i, s)| s * hann(i)).collect();
        let mut im = vec![0.0; WINDOW];
        fft(&mut re, &mut im);
        (0..WINDOW / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * 4.0 / WINDOW as f32).collect()
    }
}

/// In-place radix-2 FFT; `re.len()` is a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let (ws, wc) = (-2.0 * PI / len as f32).sin_cos();
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
                (cr, ci) = (cr * wc - ci * ws, cr * ws + ci * wc);
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_peaks_in_its_bin_and_has_its_rms() {
        let mut r = Recent::default();
        assert!(r.spectrum().is_empty() && r.amplitude() == 0.0);
        // Bin 64 of a 1024-point window, stereo.
        let wave: Vec<i16> = (0..WINDOW).flat_map(|i| {
            let s = ((2.0 * PI * 64.0 * i as f32 / WINDOW as f32).sin() * 32767.0) as i16;
            [s, s]
        }).collect();
        r.push(&wave, 2);
        let s = r.spectrum();
        assert_eq!(s.len(), 512);
        let peak = (0..512).max_by(|a, b| s[*a].total_cmp(&s[*b])).unwrap();
        assert_eq!(peak, 64);
        assert!((s[64] - 1.0).abs() < 0.05, "{}", s[64]);
        assert!((r.amplitude() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }
}
