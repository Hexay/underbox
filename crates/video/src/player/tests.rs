use super::*;
use crate::fixture::{self, PALETTE, near, pixel};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Sink {
    samples: Mutex<Vec<i16>>,
    paused: Mutex<bool>,
}

impl AudioSink for Sink {
    fn free_frames(&self) -> usize {
        48000
    }
    fn write(&self, s: &[i16], channels: usize) -> usize {
        self.samples.lock().unwrap().extend_from_slice(s);
        s.len() / channels
    }
    fn clear(&self) {
        self.samples.lock().unwrap().clear();
    }
    fn set_paused(&self, p: bool) {
        *self.paused.lock().unwrap() = p;
    }
}

#[derive(Default)]
struct Log {
    events: Mutex<Vec<String>>,
    sink: Arc<Sink>,
}

impl Events for Log {
    fn audio(&self, rate: u32, channels: u32) -> Option<Arc<dyn AudioSink>> {
        self.events.lock().unwrap().push(format!("audio {rate} {channels}"));
        Some(self.sink.clone())
    }
    fn loaded(&self, _: &Info) {
        self.events.lock().unwrap().push("loaded".into());
    }
    fn finished(&self) {
        self.events.lock().unwrap().push("finished".into());
    }
    fn repeated(&self) {
        self.events.lock().unwrap().push("repeated".into());
    }
    fn warn(&self, m: &str) {
        self.events.lock().unwrap().push(format!("warn {m}"));
    }
}

impl Log {
    fn has(&self, e: &str) -> bool {
        self.events.lock().unwrap().iter().any(|x| x == e)
    }
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

const ALL: Options = Options { decode_video: true, decode_audio: true, repeat: false, muted: false, paused: false };

#[test]
fn plays_to_the_end_presenting_frames_in_order_then_finishes() {
    let log = Arc::new(Log::default());
    let p = Player::open(fixture::h264_mkv(64, 48, 30, 9, true), ALL, log.clone());
    until("loaded", || p.is_loaded());
    let info = p.info().unwrap();
    assert!(info.has_video && info.has_audio && (info.width, info.height) == (64, 48));
    let mut shown = Vec::new();
    until("finished", || {
        shown.extend(p.take_frame().map(|f| (f.pts * 30.0).round() as usize));
        log.has("finished")
    });
    shown.extend(p.take_frame().map(|f| (f.pts * 30.0).round() as usize));
    assert!(shown.windows(2).all(|w| w[0] < w[1]) && shown.len() >= 3 && shown.last() == Some(&8), "{shown:?}");
    assert_eq!(p.time(), info.duration);
    assert_eq!(log.events.lock().unwrap()[..2], ["audio 48000 2", "loaded"]);
    let written = log.sink.samples.lock().unwrap().len() / 2;
    assert!(written > 12000 && written <= 14400, "{written}");
    assert!(p.amplitude() > 0.1 && p.spectrum().len() == 512);
    assert!(p.stats().frames == 9 && p.stats().video_seconds > 0.0);
}

#[test]
fn repeat_loops_until_cleared() {
    let log = Arc::new(Log::default());
    let p = Player::open(fixture::h264_mkv(32, 32, 30, 4, false), Options { repeat: true, ..ALL }, log.clone());
    until("repeated", || log.has("repeated"));
    assert!(!log.has("finished"));
    p.set_repeat(false);
    until("finished", || log.has("finished"));
}

#[test]
fn pause_freezes_the_clock_and_the_sink() {
    let log = Arc::new(Log::default());
    let p = Player::open(fixture::av1_webm(true), ALL, log.clone());
    until("loaded", || p.is_loaded());
    p.pause();
    let t = p.time();
    std::thread::sleep(Duration::from_millis(40));
    assert!(p.is_paused() && p.time() == t && *log.sink.paused.lock().unwrap());
    p.resume();
    until("clock running", || p.time() > t);
    assert!(!*log.sink.paused.lock().unwrap());
}

#[test]
fn seek_while_paused_shows_the_frame_at_that_time() {
    let log = Arc::new(Log::default());
    let p = Player::open(fixture::av1_webm(true), Options { paused: true, ..ALL }, log.clone());
    until("loaded", || p.is_loaded());
    p.seek(1.25);
    assert_eq!(p.time(), 1.25);
    let mut frame = None;
    until("the frame at 1.25", || {
        frame = p.take_frame().or(frame.take());
        frame.as_ref().is_some_and(|f| f.pts == 1.25)
    });
    let f = frame.unwrap();
    assert!(near(pixel(&f.rgba, f.width, 32, 16), PALETTE[10 % 8], 24));
    p.seek(99.0);
    assert_eq!(p.time(), p.info().unwrap().duration);
}

#[test]
fn without_decoders_the_clock_still_runs_to_the_end() {
    let log = Arc::new(Log::default());
    let p = Player::open(fixture::h264_mkv(32, 32, 30, 6, true), Options::default(), log.clone());
    until("finished", || log.has("finished"));
    let info = p.info().unwrap();
    assert!(info.has_video && !info.has_audio && p.take_frame().is_none());
    assert_eq!(p.time(), info.duration);
    assert_eq!(*log.events.lock().unwrap(), ["loaded", "finished"]);
}

#[test]
fn a_bad_file_warns_and_never_loads() {
    let log = Arc::new(Log::default());
    let p = Player::open(b"nope".to_vec(), ALL, log.clone());
    until("warning", || !log.events.lock().unwrap().is_empty());
    assert!(!p.is_loaded() && log.events.lock().unwrap()[0].starts_with("warn"));
    assert_eq!(p.time(), 0.0);
}
