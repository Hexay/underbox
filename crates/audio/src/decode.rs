//! Sound files (WAV, MP3, OGG Vorbis, FLAC) to [`Pcm`], through symphonia.

use crate::pcm::{Format, Pcm};
use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The source extensions a `.vsnd` can be compiled from, in lookup order.
pub const SOURCE_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac"];

#[derive(Debug)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(e: impl std::fmt::Display) -> DecodeError {
    DecodeError(e.to_string())
}

/// Decode a whole file. `ext` (without the dot) is a hint; the content is probed either way.
pub fn decode(bytes: &[u8], ext: Option<&str>) -> Result<Pcm, DecodeError> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(err)?;
    let mut reader = probed.format;
    let track = reader.default_track().ok_or_else(|| err("no audio track"))?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(err)?;

    let mut rate = params.sample_rate.unwrap_or(0);
    let mut channels: Vec<Vec<f32>> = params.channels.map(|c| vec![Vec::new(); c.count()]).unwrap_or_default();
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(err(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(err(e)),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let count = spec.channels.count();
        if channels.len() != count {
            channels = vec![Vec::new(); count];
        }
        let frames = decoded.frames();
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_planar_ref(decoded);
        for (ch, out) in channels.iter_mut().enumerate() {
            out.extend_from_slice(&buf.samples()[ch * frames..(ch + 1) * frames]);
        }
    }
    if channels.is_empty() || rate == 0 {
        return Err(err("no decodable audio"));
    }
    let mut pcm = Pcm::new(rate, channels);
    pcm.format = match ext {
        Some("mp3") => Format::Mp3,
        Some("ogg") => Format::Vorbis,
        _ => Format::Pcm16,
    };
    pcm.bits = params.bits_per_sample.unwrap_or(16);
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV of interleaved `samples`.
    pub fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn wav_decodes_exactly() {
        let pcm = decode(&wav(22050, 2, &[16384, -16384, 0, 8192]), Some("wav")).unwrap();
        assert_eq!(pcm.rate, 22050);
        assert_eq!(pcm.channels, vec![vec![0.5, 0.0], vec![-0.5, 0.25]]);
    }

    /// 0.1 s of a 0.5-amplitude 441 Hz sine, 44.1 kHz mono:
    /// `ffmpeg -f lavfi -i "aevalsrc=0.5*sin(441*2*PI*t):s=44100:d=0.1" -ac 1 sine441.{mp3,ogg}`.
    fn sine_fixture(ext: &str) -> Pcm {
        let bytes = match ext {
            "mp3" => &include_bytes!("../testdata/sine441.mp3")[..],
            _ => &include_bytes!("../testdata/sine441.ogg")[..],
        };
        decode(bytes, Some(ext)).unwrap()
    }

    #[test]
    fn mp3_and_ogg_decode_to_the_sine() {
        for ext in ["mp3", "ogg"] {
            let pcm = sine_fixture(ext);
            assert_eq!((pcm.rate, pcm.channel_count()), (44100, 1), "{ext}");
            assert!(pcm.frames() >= 4000, "{ext}: {} frames", pcm.frames());
            let peak = pcm.channels[0].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((0.4..0.6).contains(&peak), "{ext}: the 0.5 amplitude sine peaks at {peak}");
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode(b"not audio at all", None).is_err());
    }
}
