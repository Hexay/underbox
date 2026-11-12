//! The output queue between managed's mix (`SendOutput`) and a device, the feeder thread that
//! keeps it topped up, and the null device's clock.

use crate::{BLOCK, Block, RATE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Blocks mixed ahead of the device by default (about 46 ms).
pub const DEFAULT_AHEAD_BLOCKS: usize = 4;

pub fn block_duration() -> Duration {
    Duration::from_secs_f64(BLOCK as f64 / RATE as f64)
}

/// Interleaved f32 frames waiting for the device.
pub struct Queue {
    pub channels: usize,
    samples: Mutex<VecDeque<f32>>,
    drained: Condvar,
    muted: AtomicBool,
    ahead_frames: AtomicUsize,
    pushed: AtomicU64,
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

    /// Fill `out` (interleaved) from the queue, silence where it runs short or while muted.
    /// Returns the frames that came from the queue.
    pub fn pull(&self, out: &mut [f32]) -> usize {
        let mut q = self.samples.lock().unwrap();
        let n = out.len().min(q.len()) / self.channels * self.channels;
        let muted = self.muted.load(Relaxed);
        for (o, s) in out.iter_mut().zip(q.drain(..n)) {
            *o = if muted { 0.0 } else { s };
        }
        out[n..].fill(0.0);
        self.drained.notify_all();
        n / self.channels
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

/// A background thread; dropping it stops and joins the thread.
pub struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn spawn(name: &str, f: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name(name.into()).spawn(move || f(flag)).expect("spawn audio thread");
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Call `mix` (managed's `MixOneBuffer`, which ends in a `push`) whenever the queue is below its
/// mix-ahead, otherwise wait for the device.
pub fn spawn_feeder(queue: Arc<Queue>, mut mix: impl FnMut() + Send + 'static) -> Worker {
    Worker::spawn("audio feeder", move |stop| {
        while !stop.load(Relaxed) {
            if !queue.wants_more() {
                queue.wait_pull(Duration::from_millis(50));
                continue;
            }
            let before = queue.pushed();
            mix();
            if queue.pushed() == before {
                // Managed didn't send a block (an exception, or mixing paused for a hotload).
                queue.wait_pull(block_duration());
            }
        }
    })
}

/// Drain one block per block of wall time, as a sound card would, handing each to `played`.
fn spawn_clock(name: &str, queue: Arc<Queue>, mut played: impl FnMut(&[f32]) + Send + 'static) -> Worker {
    Worker::spawn(name, move |stop| {
        let mut scratch = vec![0.0; BLOCK * queue.channels];
        let mut next = Instant::now();
        while !stop.load(Relaxed) {
            queue.pull(&mut scratch);
            played(&scratch);
            next += block_duration();
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
    })
}

/// The null device: a clock with nothing to hear.
pub fn spawn_null_clock(queue: Arc<Queue>) -> Worker {
    spawn_clock("audio null device", queue, |_| {})
}

/// The disk device: a clock that appends every played frame to `out` as interleaved f32 LE, as
/// SDL's `disk` audio driver does for real engine2 (docs/differential-audio.md).
pub fn spawn_disk_clock(queue: Arc<Queue>, mut out: impl std::io::Write + Send + 'static) -> Worker {
    spawn_clock("audio disk device", queue, move |frames| {
        let bytes: Vec<u8> = frames.iter().flat_map(|s| s.to_le_bytes()).collect();
        if let Err(e) = out.write_all(&bytes).and_then(|_| out.flush()) {
            eprintln!("audio disk device: {e}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SILENT;

    fn block(v: f32) -> Block {
        [v; BLOCK]
    }

    #[test]
    fn push_interleaves_and_pull_drains() {
        let q = Queue::new(2);
        q.push(&[block(1.0), block(-1.0)]);
        assert_eq!(q.queued_frames(), BLOCK);
        let mut out = [9.0; 6];
        assert_eq!(q.pull(&mut out), 3);
        assert_eq!(out, [1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
        assert_eq!(q.queued_frames(), BLOCK - 3);
    }

    #[test]
    fn underrun_and_mute_give_silence() {
        let q = Queue::new(1);
        q.push(&[block(0.5)]);
        q.set_muted(true);
        let mut out = vec![1.0; BLOCK + 4];
        assert_eq!(q.pull(&mut out), BLOCK, "a muted device still consumes");
        assert!(out.iter().all(|&v| v == 0.0));
        let q = Queue::new(3);
        q.push(&[block(0.25)]);
        let mut out = [9.0; 3];
        q.pull(&mut out);
        assert_eq!(out, [0.25, 0.0, 0.0], "missing channels are silent");
    }

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn disk_clock_writes_played_frames_as_f32() {
        let q = Arc::new(Queue::new(2));
        q.push(&[block(0.5), block(-0.25)]);
        let sink = Sink::default();
        let clock = spawn_disk_clock(q.clone(), sink.clone());
        while sink.0.lock().unwrap().len() < BLOCK * 2 * 4 {
            q.wait_pull(Duration::from_millis(5));
        }
        drop(clock);
        let bytes = sink.0.lock().unwrap().clone();
        let first: Vec<f32> = bytes[..BLOCK * 8].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        assert!(first.as_chunks::<2>().0.iter().all(|f| *f == [0.5, -0.25]));
        assert_eq!(bytes.len() % (BLOCK * 8), 0, "whole blocks, silence after the queue ran dry");
    }

    #[test]
    fn mix_ahead_is_settable() {
        let q = Queue::new(2);
        assert_eq!(q.ahead_frames(), DEFAULT_AHEAD_BLOCKS * BLOCK);
        q.set_ahead_seconds(0.5);
        assert_eq!(q.ahead_frames(), 22050);
        q.set_ahead_seconds(0.0);
        assert_eq!(q.ahead_frames(), DEFAULT_AHEAD_BLOCKS * BLOCK);
    }

    #[test]
    fn feeder_keeps_the_queue_at_mix_ahead() {
        let q = Arc::new(Queue::new(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let (fq, fc) = (q.clone(), calls.clone());
        let feeder = spawn_feeder(q.clone(), move || {
            fc.fetch_add(1, Relaxed);
            fq.push(&[SILENT]);
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while q.pushed() < DEFAULT_AHEAD_BLOCKS as u64 && Instant::now() < deadline {
            q.wait_pull(Duration::from_millis(5));
        }
        let mut out = vec![0.0; BLOCK];
        q.pull(&mut out);
        while q.pushed() < DEFAULT_AHEAD_BLOCKS as u64 + 1 && Instant::now() < deadline {
            q.wait_pull(Duration::from_millis(5));
        }
        drop(feeder);
        assert_eq!(calls.load(Relaxed), DEFAULT_AHEAD_BLOCKS + 1, "one mix per block the device took");
        assert_eq!(q.queued_frames(), DEFAULT_AHEAD_BLOCKS * BLOCK);
    }
}
