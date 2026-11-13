//! H.264 through openh264 (built from source by `openh264-sys2`).

use super::{Frame, PtsQueue};
use crate::demux::AvcConfig;
use crate::yuv::{self, Matrix, Planes};
use openh264::decoder::{DecodedYUV, Decoder as Openh264};
use openh264::formats::YUVSource;

pub struct Decoder {
    config: AvcConfig,
    inner: Openh264,
    /// Parameter sets go in front of the first sample after a (re)start.
    primed: bool,
    pts: PtsQueue,
}

impl Decoder {
    pub fn new(config: AvcConfig) -> Result<Self, String> {
        Ok(Self { config, inner: Openh264::new().map_err(|e| format!("openh264: {e}"))?, primed: false, pts: PtsQueue::default() })
    }

    pub fn decode(&mut self, sample: &[u8], pts: f64, out: &mut Vec<Frame>) -> Result<(), String> {
        let body = self.config.annex_b(sample).ok_or("h264: sample's NAL lengths run past its end")?;
        let stream = if self.primed { body } else { [self.config.parameter_sets(), body].concat() };
        self.primed = true;
        self.pts.push(pts);
        if let Some(yuv) = self.inner.decode(&stream).map_err(|e| format!("openh264: {e}"))? {
            out.push(frame(&yuv, self.pts.pop()));
        }
        Ok(())
    }

    pub fn flush(&mut self, out: &mut Vec<Frame>) {
        let pts = &mut self.pts;
        if let Ok(rest) = self.inner.flush_remaining() {
            out.extend(rest.iter().map(|yuv| frame(yuv, pts.pop())));
        }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        *self = Self::new(std::mem::take(&mut self.config))?;
        Ok(())
    }
}

fn frame(yuv: &DecodedYUV, pts: f64) -> Frame {
    let (width, height) = yuv.dimensions();
    let (y_stride, uv_stride, _) = yuv.strides();
    let planes = Planes { y: yuv.y(), u: yuv.u(), v: yuv.v(), y_stride, uv_stride, width, height, ss_x: true, ss_y: true };
    Frame { pts, width: width as u32, height: height as u32, rgba: yuv::to_rgba(&planes, Matrix::guess(height), false) }
}
