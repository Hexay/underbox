//! Audio tracks to interleaved i16: Opus through libopus (the `opus` crate), AAC-LC through
//! symphonia.

use crate::demux::{AudioCodec, AudioTrack};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters, DecoderOptions};
use symphonia::core::formats::Packet;

/// The largest Opus frame, 120 ms at 48 kHz, per channel.
const OPUS_MAX: usize = 5760;

pub enum AudioDecoder {
    Opus { dec: opus::Decoder, channels: usize, pre_skip: usize, skip: usize },
    Aac { dec: Box<dyn symphonia::core::codecs::Decoder>, rate: u32, channels: usize },
}

impl AudioDecoder {
    pub fn new(t: &AudioTrack) -> Result<Self, String> {
        match &t.codec {
            AudioCodec::Opus { pre_skip } => {
                let (layout, channels) = match t.channels {
                    1 => (opus::Channels::Mono, 1),
                    2 => (opus::Channels::Stereo, 2),
                    n => return Err(format!("opus: {n} channels (multistream) aren't supported")),
                };
                let dec = opus::Decoder::new(48000, layout).map_err(|e| format!("opus: {e}"))?;
                Ok(Self::Opus { dec, channels, pre_skip: *pre_skip as usize, skip: *pre_skip as usize })
            }
            AudioCodec::Aac { config } => {
                let mut params = CodecParameters::new();
                params.for_codec(CODEC_TYPE_AAC).with_sample_rate(t.rate).with_extra_data(config.clone().into_boxed_slice());
                let dec = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| format!("aac: {e}"))?;
                Ok(Self::Aac { dec, rate: t.rate, channels: t.channels.clamp(1, 8) as usize })
            }
        }
    }

    pub fn rate(&self) -> u32 {
        match self {
            Self::Opus { .. } => 48000,
            Self::Aac { rate, .. } => *rate,
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            Self::Opus { channels, .. } | Self::Aac { channels, .. } => *channels,
        }
    }

    /// Append one packet's samples, interleaved at [`Self::channels`].
    pub fn decode(&mut self, data: &[u8], out: &mut Vec<i16>) -> Result<(), String> {
        match self {
            Self::Opus { dec, channels, skip, .. } => {
                let start = out.len();
                out.resize(start + OPUS_MAX * *channels, 0);
                let n = dec.decode(data, &mut out[start..], false).map_err(|e| format!("opus: {e}"))?;
                out.truncate(start + n * *channels);
                let drop = (*skip).min(n);
                out.drain(start..start + drop * *channels);
                *skip -= drop;
            }
            Self::Aac { dec, channels, .. } => {
                let buf = dec.decode(&Packet::new_from_slice(0, 0, 0, data)).map_err(|e| format!("aac: {e}"))?;
                let have = buf.spec().channels.count().max(1);
                let mut samples = SampleBuffer::<i16>::new(buf.capacity() as u64, *buf.spec());
                samples.copy_interleaved_ref(buf);
                // Remap to the channel count announced up front (the stream is sized by it).
                out.extend(samples.samples().chunks_exact(have).flat_map(|f| (0..*channels).map(move |c| f[c.min(have - 1)])));
            }
        }
        Ok(())
    }

    /// Forget decoder state (a seek); `from_start` reapplies Opus pre-skip.
    pub fn reset(&mut self, from_start: bool) {
        match self {
            Self::Opus { dec, pre_skip, skip, .. } => {
                let _ = dec.reset_state();
                *skip = if from_start { *pre_skip } else { 0 };
            }
            Self::Aac { dec, .. } => dec.reset(),
        }
    }
}
