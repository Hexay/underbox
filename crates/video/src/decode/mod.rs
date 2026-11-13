//! Decoders: video packets to RGBA frames, audio packets to i16.

mod audio;
mod av1;
mod h264;

pub use audio::AudioDecoder;

use crate::demux::VideoCodec;

/// A decoded picture, RGBA8, rows packed.
#[derive(Clone)]
pub struct Frame {
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub enum VideoDecoder {
    H264(h264::Decoder),
    Av1(av1::Decoder),
}

impl VideoDecoder {
    pub fn new(codec: &VideoCodec) -> Result<Self, String> {
        Ok(match codec {
            VideoCodec::H264(c) => Self::H264(h264::Decoder::new(c.clone())?),
            VideoCodec::Av1 => Self::Av1(av1::Decoder::new()?),
        })
    }

    /// Feed one packet (decode order); frames come out in presentation order.
    pub fn decode(&mut self, data: &[u8], pts: f64, out: &mut Vec<Frame>) -> Result<(), String> {
        match self {
            Self::H264(d) => d.decode(data, pts, out),
            Self::Av1(d) => d.decode(data, pts, out),
        }
    }

    /// End of stream: whatever is still held back.
    pub fn flush(&mut self, out: &mut Vec<Frame>) {
        match self {
            Self::H264(d) => d.flush(out),
            Self::Av1(d) => drop(d.drain(out)),
        }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        match self {
            Self::H264(d) => d.reset(),
            Self::Av1(d) => {
                d.reset();
                Ok(())
            }
        }
    }
}

/// Presentation times of packets sent but not yet output. Decoders that reorder (H.264
/// B-frames) output in presentation order, so each frame takes the smallest pending time.
#[derive(Default)]
struct PtsQueue(Vec<f64>);

impl PtsQueue {
    fn push(&mut self, pts: f64) {
        let at = self.0.partition_point(|p| *p < pts);
        self.0.insert(at, pts);
    }

    fn pop(&mut self) -> f64 {
        if self.0.is_empty() { 0.0 } else { self.0.remove(0) }
    }
}
