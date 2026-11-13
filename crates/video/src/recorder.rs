//! `CVideoRecorder`'s writer: RGBA frames to H.264 in MP4, encoded on a thread of its own.
//! Video only; docs/video.md says why it isn't the AV1/VP9 managed asks for.

use crate::encode::H264Encoder;
use mp4::{AvcConfig, FourCC, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

/// Frames waiting for the encoder before `ready` says no.
const QUEUE: usize = 4;
/// MP4 track ticks per second.
const TIMESCALE: u32 = 90_000;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_mbps: u32,
}

type Mp4Out = Mp4Writer<BufWriter<File>>;

pub struct Recorder {
    tx: Option<SyncSender<(Vec<u8>, i64)>>,
    pending: Arc<AtomicUsize>,
    frame_len: usize,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl Recorder {
    /// Create `path` now (so a bad path or size fails here) and start the encoder.
    pub fn create(path: &Path, c: Config) -> Result<Self, String> {
        let fps = c.fps.clamp(1, 240);
        let encoder = H264Encoder::new(c.width, c.height, fps, c.bitrate_mbps.clamp(1, 100) * 1_000_000)?;
        let file = File::create(path).map_err(|e| format!("recorder: {}: {e}", path.display()))?;
        let brands = ["isom", "iso2", "avc1", "mp41"].map(|b| b.parse::<FourCC>().unwrap()).to_vec();
        let config = Mp4Config { major_brand: brands[0], minor_version: 512, compatible_brands: brands, timescale: 1000 };
        let out = Mp4Writer::write_start(BufWriter::new(file), &config).map_err(|e| format!("recorder: {e}"))?;
        let (tx, rx) = sync_channel(QUEUE);
        let pending = Arc::new(AtomicUsize::new(0));
        let p = pending.clone();
        let size = (c.width as u16, c.height as u16);
        let run = move || write(rx, encoder, out, size, TIMESCALE as u64 / fps as u64, &p);
        let worker = std::thread::Builder::new().name("video-encode".into()).spawn(run).map_err(|e| e.to_string())?;
        Ok(Self { tx: Some(tx), pending, frame_len: (c.width * c.height * 4) as usize, worker: Some(worker) })
    }

    /// Room for another frame.
    pub fn ready(&self) -> bool {
        self.pending.load(Ordering::Relaxed) < QUEUE
    }

    /// Queue one RGBA frame captured at `micros` (negative: one frame after the last). Blocks
    /// while the queue is full; a frame of the wrong size is dropped.
    pub fn add_frame(&self, rgba: &[u8], micros: i64) {
        let Some(tx) = &self.tx else { return };
        if rgba.len() != self.frame_len {
            return;
        }
        self.pending.fetch_add(1, Ordering::Relaxed);
        if tx.send((rgba.to_vec(), micros)).is_err() {
            self.pending.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Encode what's queued and write the MP4's index.
    pub fn finish(mut self) -> Result<(), String> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), String> {
        self.tx = None;
        self.worker.take().map_or(Ok(()), |w| w.join().unwrap_or_else(|_| Err("recorder: encoder panicked".into())))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn write(rx: Receiver<(Vec<u8>, i64)>, mut enc: H264Encoder, mut out: Mp4Out, size: (u16, u16), frame_ticks: u64, pending: &AtomicUsize) -> Result<(), String> {
    let err = |e: mp4::Error| format!("recorder: {e}");
    let (mut index, mut track) = (0u64, false);
    // A sample's duration is known once the next one arrives.
    let mut held: Option<Mp4Sample> = None;
    for (rgba, micros) in rx {
        pending.fetch_sub(1, Ordering::Relaxed);
        let ticks = if micros >= 0 { micros as u64 * TIMESCALE as u64 / 1_000_000 } else { index * frame_ticks };
        index += 1;
        let unit = enc.encode(&rgba)?;
        if let (false, Some(sps), Some(pps)) = (track, unit.sps, unit.pps) {
            let avc = AvcConfig { width: size.0, height: size.1, seq_param_set: sps, pic_param_set: pps };
            let t = TrackConfig { track_type: TrackType::Video, timescale: TIMESCALE, language: "und".into(), media_conf: MediaConfig::AvcConfig(avc) };
            out.add_track(&t).map_err(err)?;
            track = true;
        }
        if !track || unit.sample.is_empty() {
            continue;
        }
        if let Some(mut prev) = held.take() {
            prev.duration = ticks.saturating_sub(prev.start_time).max(1) as u32;
            out.write_sample(1, &prev).map_err(err)?;
        }
        held = Some(Mp4Sample { start_time: ticks, duration: frame_ticks as u32, rendering_offset: 0, is_sync: unit.keyframe, bytes: unit.sample.into() });
    }
    if let Some(last) = held {
        out.write_sample(1, &last).map_err(err)?;
    }
    out.write_end().map_err(err)
}
