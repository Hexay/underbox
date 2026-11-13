//! One playing file: a worker thread demuxes and decodes ahead of a wall clock (worker.rs); the
//! owner reads the clock, takes due frames and steers it (pause, seek, repeat, mute).

mod clock;
#[cfg(test)]
mod tests;
mod worker;

pub use crate::decode::Frame;
use crate::spectrum::Recent;
use clock::Clock;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;

#[derive(Clone, Copy, Default)]
pub struct Options {
    /// Decode pictures (a client with a GPU); without it only the clock and audio run.
    pub decode_video: bool,
    pub decode_audio: bool,
    pub repeat: bool,
    pub muted: bool,
    pub paused: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    /// An audio track is being decoded into an [`AudioSink`].
    pub has_audio: bool,
    pub metadata: Vec<(String, String)>,
}

/// Where decoded audio goes (engine2: a `CAudioStreamManaged`).
pub trait AudioSink: Send + Sync {
    fn free_frames(&self) -> usize;
    /// Queue interleaved samples; returns the frames taken.
    fn write(&self, interleaved: &[i16], channels: usize) -> usize;
    fn clear(&self);
    fn set_paused(&self, paused: bool);
}

/// What the worker tells its owner. Called on the worker thread.
pub trait Events: Send + Sync {
    /// The audio track's format, before its first samples: where to write them, if anywhere.
    fn audio(&self, rate: u32, channels: u32) -> Option<Arc<dyn AudioSink>>;
    fn loaded(&self, info: &Info);
    fn finished(&self);
    fn repeated(&self);
    fn warn(&self, message: &str);
}

/// Running totals for managed's video overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub video_seconds: f64,
    pub audio_seconds: f64,
    pub frames: u64,
    pub packet_queue: usize,
    pub frame_queue: usize,
}

#[derive(Default)]
struct Control {
    stop: bool,
    seek: Option<f64>,
    loaded: bool,
    ended: bool,
    paused: bool,
    repeat: bool,
    muted: bool,
    clock: Clock,
}

#[derive(Default)]
struct Shared {
    control: Mutex<Control>,
    wake: Condvar,
    frames: Mutex<VecDeque<Frame>>,
    info: OnceLock<Info>,
    stats: Mutex<Stats>,
    recent: Mutex<Recent>,
    sink: Mutex<Option<Arc<dyn AudioSink>>>,
}

impl Shared {
    fn sink(&self) -> Option<Arc<dyn AudioSink>> {
        self.sink.lock().unwrap().clone()
    }
}

pub struct Player {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Player {
    /// Start playing a whole file's bytes on a new thread.
    pub fn open(bytes: Vec<u8>, options: Options, events: Arc<dyn Events>) -> Self {
        Self::load(move || Ok(bytes), options, events)
    }

    /// Start playing what `read` returns; it runs on the new thread, so file I/O doesn't block
    /// the caller. An error is reported through [`Events::warn`].
    pub fn load(read: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static, options: Options, events: Arc<dyn Events>) -> Self {
        let shared = Arc::new(Shared::default());
        {
            let mut c = shared.control.lock().unwrap();
            (c.paused, c.repeat, c.muted) = (options.paused, options.repeat, options.muted);
        }
        let s = shared.clone();
        let worker = std::thread::Builder::new().name("video".into()).spawn(move || match read() {
            Ok(bytes) => worker::run(s, bytes, options, events),
            Err(e) => events.warn(&e),
        })
        .ok();
        Self { shared, worker }
    }

    fn control<R>(&self, f: impl FnOnce(&mut Control) -> R) -> R {
        let r = f(&mut self.shared.control.lock().unwrap());
        self.shared.wake.notify_all();
        r
    }

    /// The file's description once it has been opened.
    pub fn info(&self) -> Option<Info> {
        self.shared.info.get().cloned()
    }

    pub fn is_loaded(&self) -> bool {
        self.shared.info.get().is_some()
    }

    /// Playback position in seconds.
    pub fn time(&self) -> f64 {
        let end = self.shared.info.get().map_or(f64::MAX, |i| i.duration);
        self.control(|c| c.clock.now()).min(end)
    }

    pub fn pause(&self) {
        self.control(|c| {
            c.paused = true;
            c.clock.pause();
        });
        self.shared.sink().inspect(|s| s.set_paused(true));
    }

    pub fn resume(&self) {
        self.control(|c| {
            c.paused = false;
            if c.loaded && !c.ended {
                c.clock.resume();
            }
        });
        self.shared.sink().inspect(|s| s.set_paused(false));
    }

    pub fn is_paused(&self) -> bool {
        self.control(|c| c.paused)
    }

    pub fn seek(&self, t: f64) {
        let end = self.shared.info.get().map_or(f64::MAX, |i| i.duration);
        let t = t.clamp(0.0, end.max(0.0));
        self.control(|c| {
            c.seek = Some(t);
            c.ended = false;
            c.clock.set(t);
            if c.loaded && !c.paused {
                c.clock.resume();
            }
        });
    }

    pub fn set_repeat(&self, v: bool) {
        self.control(|c| c.repeat = v);
    }

    pub fn set_muted(&self, v: bool) {
        self.control(|c| c.muted = v);
    }

    /// The newest decoded frame that is due, dropping older ones; `None` if nothing new is due.
    pub fn take_frame(&self) -> Option<Frame> {
        let now = self.time();
        let mut frames = self.shared.frames.lock().unwrap();
        let mut due = None;
        while frames.front().is_some_and(|f| f.pts <= now + 1e-3) {
            due = frames.pop_front();
        }
        drop(frames);
        self.shared.wake.notify_all();
        due
    }

    pub fn stats(&self) -> Stats {
        let mut s = *self.shared.stats.lock().unwrap();
        s.frame_queue = self.shared.frames.lock().unwrap().len();
        s
    }

    pub fn spectrum(&self) -> Vec<f32> {
        self.shared.recent.lock().unwrap().spectrum()
    }

    pub fn amplitude(&self) -> f32 {
        self.shared.recent.lock().unwrap().amplitude()
    }

    pub fn metadata(&self, key: &str) -> Option<String> {
        let key = key.to_ascii_lowercase();
        self.shared.info.get()?.metadata.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.control(|c| c.stop = true);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
