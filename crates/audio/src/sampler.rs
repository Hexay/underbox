//! `CAudioMixer`: reads one mix block at a time from a sound or stream, resampled from the
//! source rate to [`RATE`] times pitch by linear interpolation.

use crate::pcm::Pcm;
use crate::stream::Stream;
use crate::{BLOCK, Block, RATE};
use std::sync::Arc;

#[derive(Clone)]
pub enum Source {
    Pcm(Arc<Pcm>),
    Stream(Arc<Stream>),
}

impl Source {
    pub fn channels(&self) -> usize {
        match self {
            Source::Pcm(p) => p.channel_count(),
            Source::Stream(s) => s.channels,
        }
    }

    pub fn rate(&self) -> u32 {
        match self {
            Source::Pcm(p) => p.rate,
            Source::Stream(s) => s.rate,
        }
    }
}

pub struct Sampler {
    source: Source,
    /// Read position in source frames: absolute for a sound, into `window` for a stream.
    pos: f64,
    /// A stream's frames pulled but not yet passed, per channel.
    window: Vec<Vec<f32>>,
    /// Stream frames dropped from the window's front.
    consumed: u64,
    /// `EnableLooping`; `None` loops when the sound has a loop region.
    looping: Option<bool>,
    sample_end: Option<usize>,
    /// Source frames of silence still to play before the sound starts.
    delay: f64,
    time_scale: f32,
    finished: bool,
}

impl Sampler {
    pub fn new(source: Source) -> Self {
        let window = vec![Vec::new(); source.channels()];
        Self { source, pos: 0.0, window, consumed: 0, looping: None, sample_end: None, delay: 0.0, time_scale: 1.0, finished: false }
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn channels(&self) -> usize {
        self.source.channels()
    }

    pub fn should_continue(&self) -> bool {
        !self.finished
    }

    /// Frames in the sound (0 for a stream).
    pub fn sample_count(&self) -> usize {
        match &self.source {
            Source::Pcm(p) => p.frames(),
            Source::Stream(_) => 0,
        }
    }

    pub fn position(&self) -> u64 {
        self.consumed + self.pos as u64
    }

    pub fn position_frac(&self) -> f64 {
        self.pos.fract()
    }

    /// Seek a sound; a stream can't seek.
    pub fn set_position(&mut self, frame: u64, frac: f64) {
        if let Source::Pcm(_) = self.source {
            self.pos = frame as f64 + frac;
            self.finished = false;
        }
    }

    pub fn set_sample_end(&mut self, end: usize) {
        self.sample_end = (end > 0).then_some(end);
    }

    /// Positive delays the start by `frames` source frames; negative skips ahead.
    pub fn delay_or_skip(&mut self, frames: i32) {
        if frames >= 0 {
            self.delay += frames as f64;
        } else if let Source::Pcm(_) = self.source {
            self.pos += -(frames as f64);
        }
    }

    pub fn delay(&self) -> u32 {
        self.delay as u32
    }

    pub fn set_looping(&mut self, on: bool) {
        self.looping = Some(on);
    }

    pub fn set_time_scale(&mut self, scale: f32) -> bool {
        if scale.is_nan() || scale <= 0.0 {
            return false;
        }
        self.time_scale = scale;
        true
    }

    fn loop_region(&self, pcm: &Pcm) -> Option<(usize, usize)> {
        match self.looping {
            Some(false) => None,
            Some(true) => pcm.loop_region().or(Some((0, pcm.frames()))),
            None => pcm.loop_region(),
        }
    }

    /// One block into `out` (one per channel; channels past the source's are silenced).
    pub fn read(&mut self, pitch: f32, out: &mut [Block]) {
        out.iter_mut().for_each(|b| *b = crate::SILENT);
        let step = self.source.rate() as f64 / RATE as f64 * (pitch.max(0.0) * self.time_scale) as f64;
        let real_time = self.source.rate() as f64 / RATE as f64;
        let mut start = 0;
        while start < BLOCK && self.delay > 0.0 {
            self.delay = (self.delay - real_time).max(0.0);
            start += 1;
        }
        if self.finished || start == BLOCK {
            return;
        }
        match self.source.clone() {
            Source::Pcm(pcm) => self.read_pcm(&pcm, step, start, out),
            Source::Stream(s) => self.read_stream(&s, step, start, out),
        }
    }

    fn read_pcm(&mut self, pcm: &Pcm, step: f64, start: usize, out: &mut [Block]) {
        let region = self.loop_region(pcm);
        let end = self.sample_end.unwrap_or(usize::MAX).min(pcm.frames());
        for i in start..BLOCK {
            if let Some((ls, le)) = region {
                while self.pos >= le as f64 {
                    self.pos -= (le - ls) as f64;
                }
            }
            let idx = self.pos as usize;
            if region.is_none() && idx >= end {
                self.finished = true;
                return;
            }
            let next = match region {
                Some((ls, le)) if idx + 1 >= le => Some(ls),
                _ => (idx + 1 < end).then_some(idx + 1),
            };
            let t = self.pos.fract() as f32;
            for (ch, block) in out.iter_mut().enumerate().take(pcm.channel_count()) {
                let a = pcm.channels[ch][idx];
                let b = next.map_or(0.0, |n| pcm.channels[ch][n]);
                block[i] = a + (b - a) * t;
            }
            self.pos += step;
        }
    }

    fn read_stream(&mut self, stream: &Stream, step: f64, start: usize, out: &mut [Block]) {
        let wanted = (self.pos + step * (BLOCK - start) as f64).ceil() as usize + 2;
        let have = self.window.first().map_or(0, Vec::len);
        if wanted > have {
            for (w, new) in self.window.iter_mut().zip(stream.pop(wanted - have)) {
                w.extend(new);
            }
        }
        let len = self.window.first().map_or(0, Vec::len);
        let drained = stream.is_finished();
        for i in start..BLOCK {
            let idx = self.pos as usize;
            if idx >= len || (idx + 1 >= len && !drained) {
                // Underrun: hold position until more arrives.
                break;
            }
            let t = self.pos.fract() as f32;
            for (ch, block) in out.iter_mut().enumerate().take(self.window.len()) {
                let a = self.window[ch][idx];
                let b = self.window[ch].get(idx + 1).copied().unwrap_or(0.0);
                block[i] = a + (b - a) * t;
            }
            self.pos += step;
        }
        let used = (self.pos as usize).min(len);
        self.window.iter_mut().for_each(|w| drop(w.drain(..used)));
        self.consumed += used as u64;
        self.pos -= used as f64;
        if drained && self.window.first().is_none_or(Vec::is_empty) {
            self.finished = true;
        }
    }
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
