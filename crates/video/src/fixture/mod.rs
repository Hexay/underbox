//! Synthetic clips for tests: frames drawn from [`PALETTE`], encoded here (H.264, Opus) or read
//! from the checked-in AV1 stream `av1-64x64.ivf` (docs/video.md says how it was made), and
//! muxed into MP4 (the recorder) or Matroska ([`matroska`]).

pub mod matroska;

use crate::demux::AvcConfig;
use crate::encode::H264Encoder;
use matroska::Track;
use std::f64::consts::TAU;

/// Flat colours that survive lossy coding.
pub const PALETTE: [[u8; 3]; 8] = [[220, 30, 30], [30, 200, 40], [30, 40, 220], [230, 220, 30], [30, 210, 220], [210, 40, 210], [235, 235, 235], [240, 130, 20]];

/// Frame `i`: the top half `PALETTE[i % 8]`, the bottom half `PALETTE[(i + 4) % 8]`.
pub fn frame(width: u32, height: u32, i: usize) -> Vec<u8> {
    let (top, bottom) = (PALETTE[i % 8], PALETTE[(i + 4) % 8]);
    (0..height).flat_map(|y| {
        let [r, g, b] = if y < height / 2 { top } else { bottom };
        (0..width).flat_map(move |_| [r, g, b, 255])
    }).collect()
}

/// The colour frame `i` shows at (`x`, `y`), from an RGBA picture.
pub fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

/// Whether two colours are within `tolerance` per channel.
pub fn near(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
}

/// (seconds, keyframe, sample).
pub type Sample = (f64, bool, Vec<u8>);

/// `n` frames at `fps` as H.264: the avcC record and its samples.
pub fn h264(width: u32, height: u32, fps: u32, n: usize) -> (Vec<u8>, Vec<Sample>) {
    h264_of(width, height, fps, (0..n).map(|i| frame(width, height, i)))
}

/// Given RGBA frames as H.264, as [`h264`].
fn h264_of(width: u32, height: u32, fps: u32, frames: impl Iterator<Item = Vec<u8>>) -> (Vec<u8>, Vec<Sample>) {
    let mut enc = H264Encoder::new(width, height, fps, 2_000_000).unwrap();
    let (mut record, mut packets) = (Vec::new(), Vec::new());
    for (i, rgba) in frames.enumerate() {
        let unit = enc.encode(&rgba).unwrap();
        if let (true, Some(sps), Some(pps)) = (record.is_empty(), &unit.sps, &unit.pps) {
            record = AvcConfig::record(sps, pps);
        }
        if !unit.sample.is_empty() {
            packets.push((i as f64 / fps as f64, unit.keyframe, unit.sample));
        }
    }
    (record, packets)
}

/// `seconds` of a `hz` sine as 20 ms Opus packets, and the encoder's pre-skip.
pub fn opus(seconds: f64, channels: u32, hz: f64) -> (u16, Vec<(f64, bool, Vec<u8>)>) {
    let layout = if channels == 1 { opus::Channels::Mono } else { opus::Channels::Stereo };
    let mut enc = opus::Encoder::new(48000, layout, opus::Application::Audio).unwrap();
    let pre_skip = enc.get_lookahead().unwrap_or(312) as u16;
    let frames = (seconds * 50.0).round() as usize;
    let packets = (0..frames).map(|f| {
        let pcm: Vec<i16> = (0..960).flat_map(|i| {
            let s = ((TAU * hz * (f * 960 + i) as f64 / 48000.0).sin() * 16000.0) as i16;
            std::iter::repeat_n(s, channels as usize)
        }).collect();
        (f as f64 * 0.02, true, enc.encode_vec(&pcm, 4000).unwrap())
    });
    (pre_skip, packets.collect())
}

fn opus_track(seconds: f64, hz: f64) -> Track {
    let (pre_skip, packets) = opus(seconds, 2, hz);
    Track { codec_id: "A_OPUS", private: matroska::opus_head(2, pre_skip), video: None, audio: Some((48000.0, 2)), packets }
}

/// H.264 in Matroska, with a 440 Hz Opus track when `audio`.
pub fn h264_mkv(width: u32, height: u32, fps: u32, n: usize, audio: bool) -> Vec<u8> {
    let (private, packets) = h264(width, height, fps, n);
    let mut tracks = vec![Track { codec_id: "V_MPEG4/ISO/AVC", private, video: Some((width, height)), audio: None, packets }];
    if audio {
        tracks.push(opus_track(n as f64 / fps as f64, 440.0));
    }
    matroska::write("matroska", &tracks, &[("TITLE", "Synthetic")])
}

/// One second of a still (four palette quadrants) as H.264 in a `.webm`-named Matroska file: a
/// looping clip whose every frame is the same, for corpus/video's golden frame.
pub fn still_webm(width: u32, height: u32) -> Vec<u8> {
    let quadrant = |x: u32, y: u32| PALETTE[(x * 2 / width + y * 2 / height * 2) as usize];
    let still: Vec<u8> = (0..height).flat_map(|y| (0..width).flat_map(move |x| {
        let [r, g, b] = quadrant(x, y);
        [r, g, b, 255]
    })).collect();
    let (private, packets) = h264_of(width, height, 10, std::iter::repeat_n(still, 10));
    let track = Track { codec_id: "V_MPEG4/ISO/AVC", private, video: Some((width, height)), audio: None, packets };
    matroska::write("webm", &[track], &[])
}

/// H.264 in MP4, written by the recorder.
pub fn h264_mp4(path: &std::path::Path, width: u32, height: u32, fps: u32, n: usize) -> Result<(), String> {
    let config = crate::recorder::Config { width, height, fps, bitrate_mbps: 2 };
    let rec = crate::recorder::Recorder::create(path, config)?;
    for i in 0..n {
        rec.add_frame(&frame(width, height, i), (i as u64 * 1_000_000 / fps as u64) as i64);
    }
    rec.finish()
}

/// The checked-in AV1 clip's size, frame rate and frame count.
pub const AV1: (u32, u32, u32, usize) = (64, 64, 8, 16);

/// Temporal units of an IVF stream.
pub fn ivf_frames(b: &[u8]) -> Vec<Vec<u8>> {
    let header = b.get(6..8).map_or(32, |h| u16::from_le_bytes([h[0], h[1]]) as usize);
    let (mut at, mut out) = (header, Vec::new());
    while let Some(len) = b.get(at..at + 4).map(|l| u32::from_le_bytes([l[0], l[1], l[2], l[3]]) as usize) {
        let Some(unit) = b.get(at + 12..at + 12 + len) else { break };
        out.push(unit.to_vec());
        at += 12 + len;
    }
    out
}

/// The AV1 clip in WebM, with a 440 Hz Opus track when `audio`. Keyframes every 4 frames.
pub fn av1_webm(audio: bool) -> Vec<u8> {
    let (w, h, fps, _) = AV1;
    let units = ivf_frames(include_bytes!("av1-64x64.ivf"));
    let packets: Vec<(f64, bool, Vec<u8>)> = units.into_iter().enumerate().map(|(i, u)| (i as f64 / fps as f64, i % 4 == 0, u)).collect();
    let seconds = packets.len() as f64 / fps as f64;
    let mut tracks = vec![Track { codec_id: "V_AV1", private: vec![0x81, 0, 0x0C, 0], video: Some((w, h)), audio: None, packets }];
    if audio {
        tracks.push(opus_track(seconds, 440.0));
    }
    matroska::write("webm", &tracks, &[])
}
