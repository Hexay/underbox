use super::*;
use crate::SILENT;
use std::io::Write;

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
    assert_eq!(q.underruns(), (1, 4));
    let q = Queue::new(3);
    q.push(&[block(0.25)]);
    let mut out = [9.0; 3];
    q.pull(&mut out);
    assert_eq!(out, [0.25, 0.0, 0.0], "missing channels are silent");
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
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

/// A file whose first write blocks, as a busy disk's did for up to 450 ms.
struct Stalls(Sink, Option<Duration>);

impl Write for Stalls {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if let Some(d) = self.1.take() {
            std::thread::sleep(d);
        }
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn disk_clock_keeps_time_while_the_file_blocks() {
    let q = Arc::new(Queue::new(1));
    (0..40).for_each(|_| q.push(&[block(0.5)]));
    let sink = Sink::default();
    let clock = spawn_disk_clock(q.clone(), Stalls(sink.clone(), Some(Duration::from_millis(400))));
    std::thread::sleep(Duration::from_millis(250));
    let played = 40 - q.queued_frames() / BLOCK;
    drop(clock);
    assert!(played >= 10, "{played} blocks played in 250 ms (21 in real time)");
    assert!(sink.0.lock().unwrap().len() >= played * BLOCK * 4, "everything played reaches the file");
}

/// The feeder mixing at managed's pace (a couple of ms a block) while the device pulls.
fn slow_mixer(q: &Arc<Queue>, value: f32) -> Mix {
    let q = q.clone();
    Arc::new(move || {
        std::thread::sleep(Duration::from_millis(2));
        q.push(&[block(value)]);
    })
}

#[test]
fn a_device_catching_up_after_a_stall_mixes_instead_of_playing_silence() {
    let q = Arc::new(Queue::new(1));
    q.set_mixer(Some(slow_mixer(&q, 0.5)));
    let feeder = spawn_feeder(q.clone());
    while q.queued_frames() < DEFAULT_AHEAD_BLOCKS * BLOCK {
        q.wait_pull(Duration::from_millis(5));
    }
    let mut out = vec![0.0; BLOCK];
    let short = (0..20).filter(|_| q.pull(&mut out) < BLOCK || out.contains(&0.0)).count();
    drop(feeder);
    assert_eq!(short, 0, "20 blocks in a burst, faster than the feeder mixes");
    assert_eq!(q.underruns(), (0, 0));
}

#[test]
fn without_a_mixer_a_short_pull_counts_an_underrun() {
    let q = Queue::new(1);
    let mut out = vec![0.0; BLOCK];
    q.pull(&mut out);
    assert_eq!(q.underruns(), (0, 0), "nothing mixed yet: startup, not an underrun");
    q.push(&[block(0.5)]);
    q.pull(&mut out);
    q.pull(&mut out);
    assert_eq!(q.underruns(), (1, BLOCK as u64));
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
    q.set_mixer(Some(Arc::new(move || {
        fc.fetch_add(1, Relaxed);
        fq.push(&[SILENT]);
    })));
    let feeder = spawn_feeder(q.clone());
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
