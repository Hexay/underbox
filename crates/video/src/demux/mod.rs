//! Containers: MP4 (`re_mp4`) and Matroska/WebM (`matroska-demuxer`), read whole into memory and
//! flattened into one packet list in decode order, so seeking is an index search.

mod avcc;
mod mkv;
mod mp4;

pub use avcc::{AvcConfig, nal_units};
use std::ops::Range;

pub enum VideoCodec {
    Av1,
    H264(AvcConfig),
}

pub enum AudioCodec {
    /// Samples to drop from the start of the stream (OpusHead pre-skip).
    Opus { pre_skip: u32 },
    /// The AudioSpecificConfig.
    Aac { config: Vec<u8> },
}

pub struct VideoTrack {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
}

pub struct AudioTrack {
    pub codec: AudioCodec,
    pub rate: u32,
    pub channels: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Audio,
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub kind: Kind,
    /// Presentation time in seconds.
    pub pts: f64,
    pub keyframe: bool,
    range: Range<usize>,
}

pub struct Media {
    pub video: Option<VideoTrack>,
    pub audio: Option<AudioTrack>,
    /// Seconds; the container's, else the end of the last packet.
    pub duration: f64,
    /// Lowercase tag name → value (`title`, `artist`, ...).
    pub metadata: Vec<(String, String)>,
    /// Tracks that were found but can't be decoded, for the log.
    pub unsupported: Vec<String>,
    pub packets: Vec<Packet>,
    bytes: Vec<u8>,
}

impl Media {
    pub fn data(&self, p: &Packet) -> &[u8] {
        &self.bytes[p.range.clone()]
    }

    /// Where to start decoding to show time `t`: the last video keyframe at or before it when
    /// `video` is decoded, else the first packet at or after it.
    pub fn seek_index(&self, t: f64, video: bool) -> usize {
        if video && self.video.is_some() {
            let key = self.packets.iter().rposition(|p| p.kind == Kind::Video && p.keyframe && p.pts <= t + 1e-6);
            return key.unwrap_or(0);
        }
        self.packets.iter().position(|p| p.pts + 1e-6 >= t).unwrap_or(self.packets.len())
    }
}

/// Parse a whole file; Matroska by its EBML magic, MP4 otherwise.
pub fn open(bytes: Vec<u8>) -> Result<Media, String> {
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) { mkv::open(bytes) } else { mp4::open(bytes) }
}

/// Builder the container readers share: packets into one arena, ordered by decode time.
#[derive(Default)]
struct Builder {
    arena: Vec<u8>,
    packets: Vec<(f64, Packet)>,
}

impl Builder {
    /// A packet already in `bytes` (MP4 samples point into the file).
    fn at(&mut self, dts: f64, kind: Kind, pts: f64, keyframe: bool, range: Range<usize>) {
        self.packets.push((dts, Packet { kind, pts, keyframe, range }));
    }

    /// A packet copied into the arena (Matroska frames are read out).
    fn copy(&mut self, dts: f64, kind: Kind, pts: f64, keyframe: bool, data: &[u8]) {
        let start = self.arena.len();
        self.arena.extend_from_slice(data);
        self.at(dts, kind, pts, keyframe, start..self.arena.len());
    }

    fn finish(mut self, bytes: Vec<u8>, video: Option<VideoTrack>, audio: Option<AudioTrack>, duration: Option<f64>) -> Media {
        self.packets.sort_by(|a, b| a.0.total_cmp(&b.0));
        let packets: Vec<Packet> = self.packets.into_iter().map(|(_, p)| p).collect();
        let end = packets.iter().map(|p| p.pts).fold(0.0, f64::max);
        let bytes = if self.arena.is_empty() { bytes } else { self.arena };
        let duration = duration.filter(|d| *d > 0.0).unwrap_or(end);
        Media { video, audio, duration, metadata: Vec::new(), unsupported: Vec::new(), packets, bytes }
    }
}
