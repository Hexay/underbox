//! The decode thread: walks the packet list ahead of the clock. Video waits while the frame
//! queue is full, audio while it is more than [`AUDIO_LEAD`] ahead; superseded frames are dropped
//! so nobody presenting doesn't stall the audio. At the end it repeats or reports finished.

use super::{AudioSink, Events, Frame, Info, Options, Shared};
use crate::decode::{AudioDecoder, VideoDecoder};
use crate::demux::{self, Kind, Media};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Decoded frames held ahead of the clock.
const FRAME_QUEUE: usize = 4;
/// How far ahead of the clock audio is written.
const AUDIO_LEAD: f64 = 0.3;
const IDLE: Duration = Duration::from_millis(5);

enum Step {
    Decode(usize, bool),
    Skip,
    Seek(f64),
    Flush,
    Repeat,
    Finished,
}

struct Worker {
    shared: Arc<Shared>,
    events: Arc<dyn Events>,
    media: Media,
    video: Option<VideoDecoder>,
    audio: Option<(AudioDecoder, Arc<dyn AudioSink>)>,
    cursor: usize,
    /// After a seek: audio before this is dropped (video's late frames drop themselves).
    skip_until: f64,
    flushed: bool,
    pcm: Vec<i16>,
    decoded: Vec<Frame>,
}

pub(super) fn run(shared: Arc<Shared>, bytes: Vec<u8>, options: Options, events: Arc<dyn Events>) {
    let media = match demux::open(bytes) {
        Ok(m) => m,
        Err(e) => return events.warn(&e),
    };
    for u in &media.unsupported {
        events.warn(&format!("video: skipping unsupported track {u}"));
    }
    if media.video.is_none() && media.audio.is_none() {
        return events.warn("video: no track that can be played");
    }
    let warn = |e: String| events.warn(&e);
    let video = media.video.as_ref().filter(|_| options.decode_video).and_then(|t| VideoDecoder::new(&t.codec).map_err(warn).ok());
    let audio = media.audio.as_ref().filter(|_| options.decode_audio).and_then(|t| AudioDecoder::new(t).map_err(warn).ok());
    let audio = audio.and_then(|a| Some((events.audio(a.rate(), a.channels() as u32)?, a))).map(|(sink, a)| (a, sink));
    if let Some((_, sink)) = &audio {
        sink.set_paused(options.paused);
        *shared.sink.lock().unwrap() = Some(sink.clone());
    }
    let (width, height) = media.video.as_ref().map_or((0, 0), |v| (v.width, v.height));
    let info = Info { duration: media.duration, width, height, has_video: media.video.is_some(), has_audio: audio.is_some(), metadata: media.metadata.clone() };
    let _ = shared.info.set(info.clone());
    {
        let mut c = shared.control.lock().unwrap();
        c.loaded = true;
        if !c.paused {
            c.clock.resume();
        }
    }
    events.loaded(&info);
    let mut w = Worker { shared, events, media, video, audio, cursor: 0, skip_until: 0.0, flushed: false, pcm: Vec::new(), decoded: Vec::new() };
    w.run();
}

impl Worker {
    fn run(&mut self) {
        let shared = self.shared.clone();
        loop {
            let step = {
                let mut c = shared.control.lock().unwrap();
                loop {
                    if c.stop {
                        return;
                    }
                    if let Some(step) = self.next(&mut c) {
                        break step;
                    }
                    c = shared.wake.wait_timeout(c, IDLE).unwrap().0;
                }
            };
            match step {
                Step::Decode(i, muted) => self.decode(i, muted),
                Step::Skip => self.cursor += 1,
                Step::Seek(t) => self.reposition(t),
                Step::Flush => {
                    if let Some(v) = &mut self.video {
                        v.flush(&mut self.decoded);
                    }
                    self.queue_decoded();
                    self.flushed = true;
                }
                Step::Repeat => {
                    self.reposition(0.0);
                    self.events.repeated();
                }
                Step::Finished => self.events.finished(),
            }
        }
    }

    /// What to do now, or `None` to wait.
    fn next(&mut self, c: &mut super::Control) -> Option<Step> {
        if let Some(t) = c.seek.take() {
            return Some(Step::Seek(t));
        }
        let now = c.clock.now();
        let Some(p) = self.media.packets.get(self.cursor) else {
            if !self.flushed {
                return Some(Step::Flush);
            }
            if now + 1e-6 < self.media.duration || c.ended {
                return None;
            }
            if c.repeat {
                c.clock.set(0.0);
                return Some(Step::Repeat);
            }
            c.ended = true;
            c.clock.pause();
            c.clock.set(self.media.duration);
            return Some(Step::Finished);
        };
        match p.kind {
            Kind::Video if self.video.is_some() => {
                let mut frames = self.shared.frames.lock().unwrap();
                while frames.len() >= 2 && frames[1].pts <= now {
                    frames.pop_front();
                }
                (frames.len() < FRAME_QUEUE).then_some(Step::Decode(self.cursor, c.muted))
            }
            Kind::Audio if self.audio.is_some() => {
                let room = self.audio.as_ref().is_some_and(|(a, s)| s.free_frames() >= a.rate() as usize / 10);
                (room && p.pts <= now + AUDIO_LEAD).then_some(Step::Decode(self.cursor, c.muted))
            }
            _ => Some(Step::Skip),
        }
    }

    fn decode(&mut self, i: usize, muted: bool) {
        self.cursor += 1;
        self.shared.stats.lock().unwrap().packet_queue = self.media.packets.len() - self.cursor;
        let p = self.media.packets[i].clone();
        let started = Instant::now();
        match p.kind {
            Kind::Video => {
                let Some(v) = &mut self.video else { return };
                if let Err(e) = v.decode(self.media.data(&p), p.pts, &mut self.decoded) {
                    self.events.warn(&e);
                }
                self.shared.stats.lock().unwrap().video_seconds += started.elapsed().as_secs_f64();
                self.queue_decoded();
            }
            Kind::Audio => {
                let Some((a, sink)) = &mut self.audio else { return };
                self.pcm.clear();
                if let Err(e) = a.decode(self.media.data(&p), &mut self.pcm) {
                    self.events.warn(&e);
                }
                let ch = a.channels();
                let late = ((self.skip_until - p.pts).max(0.0) * a.rate() as f64) as usize * ch;
                self.pcm.drain(..late.min(self.pcm.len()));
                self.shared.recent.lock().unwrap().push(&self.pcm, ch);
                if muted {
                    self.pcm.fill(0);
                }
                sink.write(&self.pcm, ch);
                self.shared.stats.lock().unwrap().audio_seconds += started.elapsed().as_secs_f64();
            }
        }
    }

    fn queue_decoded(&mut self) {
        if self.decoded.is_empty() {
            return;
        }
        self.shared.stats.lock().unwrap().frames += self.decoded.len() as u64;
        self.shared.frames.lock().unwrap().extend(self.decoded.drain(..));
    }

    /// Restart decoding so that time `t` shows next.
    fn reposition(&mut self, t: f64) {
        self.cursor = self.media.seek_index(t, self.video.is_some());
        self.skip_until = t;
        self.flushed = false;
        self.decoded.clear();
        self.shared.frames.lock().unwrap().clear();
        if let Some(v) = &mut self.video
            && let Err(e) = v.reset()
        {
            self.events.warn(&e);
        }
        if let Some((a, sink)) = &mut self.audio {
            a.reset(t == 0.0);
            sink.clear();
        }
        self.shared.recent.lock().unwrap().clear();
    }
}
