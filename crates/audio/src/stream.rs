//! `CAudioStreamManaged`: interleaved i16 that game code (or voice chat) writes, and a sampler
//! drains at the stream's own rate.

use std::collections::VecDeque;
use std::sync::Mutex;

/// Seconds of audio a stream holds before writes are dropped.
const CAPACITY_SECONDS: usize = 2;

#[derive(Default)]
struct State {
    queue: VecDeque<i16>,
    paused: bool,
    closed: bool,
}

pub struct Stream {
    pub channels: usize,
    pub rate: u32,
    state: Mutex<State>,
}

impl Stream {
    pub fn new(channels: usize, rate: u32) -> Self {
        Self { channels: channels.max(1), rate, state: Mutex::default() }
    }

    fn capacity(&self) -> usize {
        self.rate as usize * CAPACITY_SECONDS
    }

    /// Queue `frames` frames of interleaved samples; what doesn't fit is dropped. Returns the
    /// frames queued.
    pub fn write(&self, samples: &[i16], frames: usize, channels: usize) -> usize {
        let mut s = self.state.lock().unwrap();
        let fit = frames.min(self.capacity().saturating_sub(s.queue.len() / self.channels));
        let channels = channels.max(1);
        for frame in samples.chunks_exact(channels).take(fit) {
            s.queue.extend((0..self.channels).map(|c| frame[c.min(channels - 1)]));
        }
        fit
    }

    pub fn queued_frames(&self) -> usize {
        self.state.lock().unwrap().queue.len() / self.channels
    }

    pub fn free_frames(&self) -> usize {
        self.capacity().saturating_sub(self.queued_frames())
    }

    /// Drop everything queued (a seek in what feeds it).
    pub fn clear(&self) {
        self.state.lock().unwrap().queue.clear();
    }

    pub fn set_paused(&self, paused: bool) {
        self.state.lock().unwrap().paused = paused;
    }

    /// No more writes are coming: a sampler finishes once the queue drains.
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }

    /// Up to `max` frames, one Vec per channel. Nothing while paused.
    pub fn pop(&self, max: usize) -> Vec<Vec<f32>> {
        let mut s = self.state.lock().unwrap();
        let n = if s.paused { 0 } else { max.min(s.queue.len() / self.channels) };
        let mut out = vec![Vec::with_capacity(n); self.channels];
        for (i, v) in s.queue.drain(..n * self.channels).enumerate() {
            out[i % self.channels].push(v as f32 / 32768.0);
        }
        out
    }

    /// Closed and drained.
    pub fn is_finished(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.closed && s.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_queue_and_pop_planar() {
        let s = Stream::new(2, 100);
        assert_eq!(s.write(&[16384, -16384, 0, 8192], 2, 2), 2);
        assert_eq!(s.queued_frames(), 2);
        assert_eq!(s.pop(10), vec![vec![0.5, 0.0], vec![-0.5, 0.25]]);
        assert_eq!(s.queued_frames(), 0);
    }

    #[test]
    fn capacity_drops_overflow_and_pause_holds() {
        let s = Stream::new(1, 10);
        assert_eq!(s.write(&[1; 30], 30, 1), 20, "two seconds at 10 Hz");
        assert_eq!(s.free_frames(), 0);
        s.set_paused(true);
        assert!(s.pop(5)[0].is_empty());
        s.set_paused(false);
        assert_eq!(s.pop(5)[0].len(), 5);
    }

    #[test]
    fn mono_writes_fill_every_channel_and_close_finishes_when_drained() {
        let s = Stream::new(2, 100);
        s.write(&[16384], 1, 1);
        s.close();
        assert!(!s.is_finished());
        assert_eq!(s.pop(1), vec![vec![0.5], vec![0.5]]);
        assert!(s.is_finished());
    }
}
