//! The threads around a [`Queue`]: the feeder that keeps it topped up, and the null and disk
//! devices' clocks.

use super::{Queue, block_duration};
use crate::BLOCK;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Background threads; dropping it stops and joins them, in order.
pub struct Worker {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

fn spawn_named(name: &str, f: impl FnOnce() + Send + 'static) -> JoinHandle<()> {
    std::thread::Builder::new().name(name.into()).spawn(f).expect("spawn audio thread")
}

impl Worker {
    fn spawn(name: &str, f: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        Self { stop, threads: vec![spawn_named(name, move || f(flag))] }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        self.threads.drain(..).for_each(|t| {
            let _ = t.join();
        });
    }
}

/// Mix (through the queue's mixer) whenever the queue is below its mix-ahead, otherwise wait for
/// the device.
pub fn spawn_feeder(queue: Arc<Queue>) -> Worker {
    Worker::spawn("audio feeder", move |stop| {
        while !stop.load(Relaxed) {
            if !queue.wants_more() {
                queue.wait_pull(Duration::from_millis(50));
            } else if !queue.mix_one() {
                queue.wait_pull(block_duration());
            }
        }
    })
}

/// Drain one block per block of wall time, as a sound card would, handing each to `played`.
/// After a stall it catches up; `pull` mixes what the feeder hasn't, so that's sound, not silence.
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
pub fn spawn_disk_clock(queue: Arc<Queue>, out: impl std::io::Write + Send + 'static) -> Worker {
    // Off the clock's thread: a busy disk blocks writes for hundreds of ms (docs/audio.md, "Per frame").
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let writer = spawn_named("audio disk writer", move || write_out(rx, out));
    let mut worker = spawn_clock("audio disk device", queue, move |frames| {
        let _ = tx.send(frames.iter().flat_map(|s| s.to_le_bytes()).collect());
    });
    worker.threads.push(writer);
    worker
}

/// Write what arrives, flushing whenever it has caught up; ends when the clock does.
fn write_out(rx: mpsc::Receiver<Vec<u8>>, mut out: impl std::io::Write) {
    while let Ok(mut bytes) = rx.recv() {
        bytes.extend(rx.try_iter().flatten());
        if let Err(e) = out.write_all(&bytes).and_then(|_| out.flush()) {
            eprintln!("audio disk device: {e}");
        }
    }
}
