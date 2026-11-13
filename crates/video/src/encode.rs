//! RGBA frames to H.264 access units (openh264), in MP4/Matroska's length-prefixed form.

use crate::demux::nal_units;
use openh264::OpenH264API;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, FrameType, UsageType};
use openh264::formats::{RgbaSliceU8, YUVBuffer};

pub struct H264Encoder {
    inner: Encoder,
    width: usize,
    height: usize,
}

/// One encoded frame.
pub struct Unit {
    /// The parameter sets, when this frame starts a new sequence.
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
    /// NAL units with 4-byte big-endian lengths; empty for a skipped frame.
    pub sample: Vec<u8>,
    pub keyframe: bool,
}

impl H264Encoder {
    /// `width` and `height` must be even.
    pub fn new(width: u32, height: u32, fps: u32, bitrate_bps: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(format!("h264: {width}x{height} isn't an even, non-empty size"));
        }
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(fps.clamp(1, 240) as f32))
            .usage_type(UsageType::ScreenContentRealTime);
        let inner = Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| format!("openh264: {e}"))?;
        Ok(Self { inner, width: width as usize, height: height as usize })
    }

    /// Encode one RGBA frame (`width * height * 4` bytes).
    pub fn encode(&mut self, rgba: &[u8]) -> Result<Unit, String> {
        let yuv = YUVBuffer::from_rgba8_source(RgbaSliceU8::new(rgba, (self.width, self.height)));
        let bits = self.inner.encode(&yuv).map_err(|e| format!("openh264: {e}"))?;
        let mut unit = Unit { sps: None, pps: None, sample: Vec::new(), keyframe: matches!(bits.frame_type(), FrameType::IDR | FrameType::I) };
        for layer in (0..bits.num_layers()).filter_map(|l| bits.layer(l)) {
            for nal in (0..layer.nal_count()).filter_map(|n| layer.nal_unit(n)).flat_map(nal_units) {
                match nal.first().map(|b| b & 0x1F) {
                    Some(7) => unit.sps = Some(nal.to_vec()),
                    Some(8) => unit.pps = Some(nal.to_vec()),
                    Some(_) => {
                        unit.sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                        unit.sample.extend_from_slice(nal);
                    }
                    None => {}
                }
            }
        }
        Ok(unit)
    }
}
