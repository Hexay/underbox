//! The output queue between managed's mix (`SendOutput`) and a device. Managed's `MixOneBuffer`
//! is called from two places, as its `MixingThread` expects: the feeder thread keeps the queue at
//! its mix-ahead, and a device that finds the queue short mixes the rest itself (docs/audio.md).

mod threads;
#[cfg(test)]
mod tests;

pub use threads::{Worker, spawn_disk_clock, spawn_feeder, spawn_null_clock};

use crate::{BLOCK, Block, RATE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Blocks mixed ahead of the device by default (about 46 ms).
pub const DEFAULT_AHEAD_BLOCKS: usize = 4;

pub fn block_duration() -> Duration {
    Duration::from_secs_f64(BLOCK as f64 / RATE as f64)
}

/// Managed's `MixOneBuffer`: mixes one block and hands it to [`Queue::push`].
pub type Mix = Arc<dyn Fn() + Send + Sync>;

/// Interleaved f32 frames waiting for the device.
pub struct Queue {
    pub channels: usize,
    samples: Mutex<VecDeque<f32>>,
    drained: Condvar,
    muted: AtomicBool,
    ahead_frames: AtomicUsize,
    pushed: AtomicU64,
    mixer: Mutex<Option<Mix>>,
    underruns: AtomicU64,
    underrun_frames: AtomicU64,
}

impl Queue {
    pub fn new(channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            samples: Mutex::default(),
            drained: Condvar::new(),
            muted: AtomicBool::new(false),
            ahead_frames: AtomicUsize::new(DEFAULT_AHEAD_BLOCKS * BLOCK),
            pushed: AtomicU64::new(0),
            mixer: Mutex::new(None),
            underruns: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
        }
    }

    /// Interleave one mix block. Channels the queue has but `blocks` lacks are silent.
    pub fn push(&self, blocks: &[Block]) {
        let mut q = self.samples.lock().unwrap();
        for i in 0..BLOCK {
            q.extend((0..self.channels).map(|c| blocks.get(c).map_or(0.0, |b| b[i])));
        }
        self.pushed.fetch_add(1, Relaxed);
    }

    /// Blocks pushed so far.
    pub fn pushed(&self) -> u64 {
        self.pushed.load(Relaxed)
    }

    /// Who mixes into this queue; `None` stops mixing, after waiting out a mix in progress.
    pub fn set_mixer(&self, mix: Option<Mix>) {
        *self.mixer.lock().unwrap() = mix;
    }

    /// Mix one block on this thread. False if there's no mixer or it sent nothing (an exception,
    /// or mixing paused for a hotload).
    pub fn mix_one(&self) -> bool {
        let mixer = self.mixer.lock().unwrap();
        let Some(mix) = mixer.as_ref() else { return false };
        let before = self.pushed();
        mix();
        self.pushed() != before
    }

    /// The device's side: fill `out` (interleaved) from the queue, first mixing on this thread
    /// what the feeder hasn't, then silence where it still runs short or while muted.
    /// Returns the frames that came from the queue.
    pub fn pull(&self, out: &mut [f32]) -> usize {
        let wanted = out.len() / self.channels;
        while self.queued_frames() < wanted && self.mix_one() {}
        let mut q = self.samples.lock().unwrap();
        let n = out.len().min(q.len()) / self.channels * self.channels;
        let muted = self.muted.load(Relaxed);
        for (o, s) in out.iter_mut().zip(q.drain(..n)) {
            *o = if muted { 0.0 } else { s };
        }
        out[n..].fill(0.0);
        self.drained.notify_all();
        let got = n / self.channels;
        if got < wanted && self.pushed() > 0 {
            self.underruns.fetch_add(1, Relaxed);
            self.underrun_frames.fetch_add((wanted - got) as u64, Relaxed);
        }
        got
    }

    /// Short pulls since mixing began, and the silent frames they played.
    pub fn underruns(&self) -> (u64, u64) {
        (self.underruns.load(Relaxed), self.underrun_frames.load(Relaxed))
    }

    pub fn queued_frames(&self) -> usize {
        self.samples.lock().unwrap().len() / self.channels
    }

    pub fn clear(&self) {
        self.samples.lock().unwrap().clear();
        self.drained.notify_all();
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Relaxed)
    }

    /// Mix this far ahead of the device; 0 restores the default.
    pub fn set_ahead_seconds(&self, seconds: f32) {
        let frames = if seconds > 0.0 { (seconds * RATE as f32) as usize } else { DEFAULT_AHEAD_BLOCKS * BLOCK };
        self.ahead_frames.store(frames.max(BLOCK), Relaxed);
    }

    pub fn ahead_frames(&self) -> usize {
        self.ahead_frames.load(Relaxed)
    }

    pub fn wants_more(&self) -> bool {
        self.queued_frames() < self.ahead_frames()
    }

    /// Wait for the device to pull, at most `timeout`.
    pub fn wait_pull(&self, timeout: Duration) {
        let q = self.samples.lock().unwrap();
        let _ = self.drained.wait_timeout(q, timeout).unwrap();
    }

    /// Wait until the device has played everything queued, at most `timeout`.
    pub fn wait_empty(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut q = self.samples.lock().unwrap();
        while !q.is_empty() && Instant::now() < deadline {
            q = self.drained.wait_timeout(q, deadline - Instant::now()).unwrap().0;
        }
    }
}
