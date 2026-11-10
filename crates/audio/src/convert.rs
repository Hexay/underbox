//! Rate conversion between the output queue (44.1 kHz) and a device running at another rate,
//! linear over interleaved frames.

use crate::output::Queue;

pub struct Converter {
    channels: usize,
    /// Queue frames per device frame.
    step: f64,
    pos: f64,
    /// Interleaved frames pulled from the queue, the first being the one `pos` is past.
    frames: Vec<f32>,
}

impl Converter {
    pub fn new(channels: usize, device_rate: u32) -> Self {
        let channels = channels.max(1);
        Self { channels, step: crate::RATE as f64 / device_rate.max(1) as f64, pos: 0.0, frames: Vec::new() }
    }

    /// Fill `out` (interleaved, device rate) from `queue`.
    pub fn fill(&mut self, queue: &Queue, out: &mut [f32]) {
        let c = self.channels;
        let out_frames = out.len() / c;
        let need = (self.pos + self.step * out_frames as f64).ceil() as usize + 2;
        let have = self.frames.len() / c;
        if need > have {
            let start = self.frames.len();
            self.frames.resize(need * c, 0.0);
            queue.pull(&mut self.frames[start..]);
        }
        for (f, frame) in out.chunks_exact_mut(c).enumerate() {
            let p = self.pos + self.step * f as f64;
            let (i, t) = (p as usize, p.fract() as f32);
            for (ch, o) in frame.iter_mut().enumerate() {
                let a = self.frames[i * c + ch];
                *o = a + (self.frames[(i + 1) * c + ch] - a) * t;
            }
        }
        self.pos += self.step * out_frames as f64;
        let used = self.pos as usize;
        self.frames.drain(..used * c);
        self.pos -= used as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BLOCK;

    #[test]
    fn half_rate_device_takes_every_other_frame() {
        let q = Queue::new(1);
        q.push(&[std::array::from_fn(|i| i as f32)]);
        let mut conv = Converter::new(1, crate::RATE / 2);
        let mut out = [0.0; 8];
        conv.fill(&q, &mut out);
        assert_eq!(out, [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0]);
        conv.fill(&q, &mut out);
        assert_eq!(out[0], 16.0, "continues where the last callback stopped");
    }

    #[test]
    fn faster_device_interpolates_and_keeps_stereo_pairs() {
        let q = Queue::new(2);
        q.push(&[std::array::from_fn(|i| i as f32), std::array::from_fn(|i| -(i as f32))]);
        let mut conv = Converter::new(2, crate::RATE * 2);
        let mut out = [0.0; 8];
        conv.fill(&q, &mut out);
        assert_eq!(out, [0.0, 0.0, 0.5, -0.5, 1.0, -1.0, 1.5, -1.5]);
        assert!(q.queued_frames() < BLOCK);
    }
}
