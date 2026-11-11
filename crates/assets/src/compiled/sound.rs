//! Compiled sounds (`.vsnd_c`): a header (binary DATA in versions 1–4, CTRL KV3 `m_vSound` in 5)
//! and the stream, which trails the container at its header's file size. PCM and ADPCM streams
//! are bare samples that [`Sound::playable`] wraps in a RIFF header; MP3 streams are whole files.

use super::kv::{self, float, int, int_value, obj, text};
use super::{Error, format};
use ::kv::{Object, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Pcm16,
    Pcm8,
    Mp3,
    Adpcm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub codec: Codec,
    pub rate: u32,
    pub channels: u16,
    pub sample_count: u32,
    pub duration: f32,
    /// In sample frames; negative for none.
    pub loop_start: i32,
    /// 0 for the end of the sound.
    pub loop_end: i32,
    /// ADPCM's `WAVEFORMATEX` (the RIFF `fmt ` chunk); empty otherwise.
    pub format_header: Vec<u8>,
    pub stream: Vec<u8>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn codec_named(s: &str) -> Option<Codec> {
    match s {
        "PCM16" => Some(Codec::Pcm16),
        "PCM8" => Some(Codec::Pcm8),
        "MP3" => Some(Codec::Mp3),
        "ADPCM" => Some(Codec::Adpcm),
        _ => None,
    }
}

/// `m_vSound` (version 5's CTRL), or a KV3 DATA block with the same fields.
fn from_kv3(s: &Object) -> Option<Sound> {
    let codec = match s.get("m_nFormat")? {
        Value::String(f) => codec_named(f)?,
        v => [Codec::Pcm16, Codec::Pcm8, Codec::Mp3, Codec::Adpcm].get(usize::try_from(kv::int_of(v)?).ok()?).copied()?,
    };
    Some(Sound {
        codec,
        rate: int(s, "m_nRate") as u32,
        channels: int(s, "m_nChannels") as u16,
        sample_count: int(s, "m_nSampleCount") as u32,
        duration: float(s, "m_flDuration"),
        loop_start: kv::int_or(s, "m_nLoopStart", -1) as i32,
        loop_end: int(s, "m_nLoopEnd") as i32,
        format_header: Vec::new(),
        stream: Vec::new(),
    })
}

/// Versions 1–4: a packed `VSound_t`. Returns the sound and its stream size.
fn from_binary(d: &[u8], version: u16) -> Option<(Sound, u32)> {
    let (codec, channels, rate) = if version >= 4 {
        let codec = [Codec::Pcm16, Codec::Pcm8, Codec::Mp3, Codec::Adpcm].get(usize::from(*d.get(2)?)).copied()?;
        (codec, u16::from(*d.get(3)?), u32::from(u16::from_le_bytes([d[0], d[1]])))
    } else {
        let packed = u32_at(d, 0)?;
        let bits = |at: u32, n: u32| (packed >> at) & ((1 << n) - 1);
        let codec = match (bits(0, 2), bits(12, 2), bits(2, 5)) {
            (1, _, _) => Codec::Mp3,
            (0, 2, _) => Codec::Adpcm,
            (0, _, 8) => Codec::Pcm8,
            (0 | 2, _, _) => Codec::Pcm16,
            _ => return None,
        };
        (codec, bits(7, 2) as u16, bits(14, 16))
    };
    let loop_start = u32_at(d, 4)? as i32;
    let sample_count = u32_at(d, 8)?;
    let duration = f32::from_bits(u32_at(d, 12)?);
    // Offsets count from their own field.
    let (header_offset, header_size, stream_size) = (u32_at(d, 20)? as usize, u32_at(d, 24)? as usize, u32_at(d, 28)?);
    let loop_end_at = 32 + if version >= 1 { 8 } else { 0 } + if version >= 2 { 4 } else { 0 };
    let loop_end = if version >= 4 { u32_at(d, loop_end_at)? as i32 } else { 0 };
    let format_header = if header_size > 0 { d.get(20 + header_offset..20 + header_offset + header_size)?.to_vec() } else { Vec::new() };
    let loop_start = if version < 3 && codec == Codec::Mp3 { -1 } else { loop_start };
    Some((Sound { codec, rate, channels, sample_count, duration, loop_start, loop_end, format_header, stream: Vec::new() }, stream_size))
}

pub fn read(file: &[u8]) -> Result<Sound, Error> {
    let version = file.get(6..8).map_or(0, |v| u16::from_le_bytes([v[0], v[1]]));
    let ctrl = crate::container::block(file, b"CTRL")?.and_then(|b| crate::kv3bin::read(b).ok());
    let ctrl = ctrl.as_ref().and_then(Value::as_object).filter(|c| c.str("_class").is_some_and(|k| k.starts_with("CVoiceContainer")));
    let data = crate::container::block(file, b"DATA")?.unwrap_or_default();
    let (mut sound, stream_size) = match ctrl.and_then(|c| from_kv3(kv::object(c, "m_vSound"))) {
        Some(s) => (s, None),
        None => match crate::kv3bin::read(data).ok().as_ref().and_then(Value::as_object) {
            Some(o) => (from_kv3(o.get("m_vSound").and_then(Value::as_object).unwrap_or(o)).ok_or_else(|| format("vsnd KV3 has no sound"))?, None),
            None => from_binary(data, version).map(|(s, n)| (s, Some(n))).ok_or_else(|| format("vsnd DATA too short"))?,
        },
    };
    let start = u32_at(file, 0).ok_or_else(|| format("truncated header"))? as usize;
    let rest = file.get(start..).unwrap_or_default();
    sound.stream = rest[..stream_size.map_or(rest.len(), |n| (n as usize).min(rest.len()))].to_vec();
    Ok(sound)
}

impl Sound {
    /// A file a decoder takes, and its extension: the stream as is when it already is a file
    /// (MP3, or a RIFF/Ogg/FLAC payload), else PCM/ADPCM under a RIFF header.
    pub fn playable(&self) -> (Vec<u8>, &'static str) {
        let s = &self.stream;
        if s.starts_with(b"RIFF") {
            return (s.clone(), "wav");
        }
        if s.starts_with(b"OggS") {
            return (s.clone(), "ogg");
        }
        if s.starts_with(b"fLaC") {
            return (s.clone(), "flac");
        }
        let fmt = match self.codec {
            Codec::Mp3 => return (s.clone(), "mp3"),
            Codec::Adpcm => self.format_header.clone(),
            Codec::Pcm16 | Codec::Pcm8 => {
                let bits: u16 = if self.codec == Codec::Pcm8 { 8 } else { 16 };
                let block = self.channels * bits / 8;
                let mut f = Vec::with_capacity(16);
                f.extend(1u16.to_le_bytes());
                f.extend(self.channels.to_le_bytes());
                f.extend(self.rate.to_le_bytes());
                f.extend((self.rate * u32::from(block)).to_le_bytes());
                f.extend(block.to_le_bytes());
                f.extend(bits.to_le_bytes());
                f
            }
        };
        let mut out = Vec::with_capacity(28 + fmt.len() + s.len());
        out.extend(b"RIFF");
        out.extend(((20 + fmt.len() + s.len()) as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend((fmt.len() as u32).to_le_bytes());
        out.extend(&fmt);
        out.extend(b"data");
        out.extend((s.len() as u32).to_le_bytes());
        out.extend(s);
        (out, "wav")
    }
}

/// A container followed by `stream`, the header's file size pointing at the stream.
fn with_stream(version: u16, blocks: Vec<([u8; 4], Vec<u8>)>, stream: &[u8]) -> Vec<u8> {
    let mut out = super::write(version, &[], blocks);
    out.extend_from_slice(stream);
    out
}

/// Version 5: empty DATA, the header in CTRL (as current compilers write).
pub fn write_v5(s: &Sound) -> Vec<u8> {
    let name = match s.codec {
        Codec::Pcm16 => "PCM16",
        Codec::Pcm8 => "PCM8",
        Codec::Mp3 => "MP3",
        Codec::Adpcm => "ADPCM",
    };
    let sound = obj(vec![
        ("m_nFormat", text(name)),
        ("m_nRate", int_value(s.rate)),
        ("m_nChannels", int_value(s.channels)),
        ("m_nSampleCount", int_value(s.sample_count)),
        ("m_flDuration", Value::Double(s.duration.into())),
        ("m_nLoopStart", int_value(s.loop_start)),
        ("m_nLoopEnd", int_value(s.loop_end)),
        ("m_nStreamingSize", int_value(s.stream.len() as i64)),
    ]);
    let ctrl = obj(vec![("_class", text("CVoiceContainerDefault")), ("m_vSound", sound)]);
    with_stream(5, vec![(*b"CTRL", super::kv3(&ctrl)), (*b"DATA", Vec::new())], &s.stream)
}

/// Version 4: the packed binary header, an ADPCM format header after it.
pub fn write_v4(s: &Sound) -> Vec<u8> {
    let codec = [Codec::Pcm16, Codec::Pcm8, Codec::Mp3, Codec::Adpcm].iter().position(|c| *c == s.codec).unwrap_or(0) as u8;
    let mut d = Vec::new();
    d.extend((s.rate as u16).to_le_bytes());
    d.extend([codec, s.channels as u8]);
    d.extend(s.loop_start.to_le_bytes());
    d.extend(s.sample_count.to_le_bytes());
    d.extend(s.duration.to_bits().to_le_bytes());
    d.extend(0u32.to_le_bytes());
    // m_pHeader: offset from this field to just past the fixed fields (48 bytes in).
    let header_offset: u32 = if s.format_header.is_empty() { 0 } else { 48 - 20 };
    d.extend(header_offset.to_le_bytes());
    d.extend((s.format_header.len() as u32).to_le_bytes());
    d.extend((s.stream.len() as u32).to_le_bytes());
    d.extend([0u8; 12]);
    d.extend(s.loop_end.to_le_bytes());
    d.extend(&s.format_header);
    with_stream(4, vec![(*b"DATA", d)], &s.stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(codec: Codec) -> Sound {
        let stream: Vec<u8> = (0..64i16).flat_map(|i| (i * 256).to_le_bytes()).collect();
        Sound { codec, rate: 22050, channels: 2, sample_count: 32, duration: 32.0 / 22050.0, loop_start: 4, loop_end: 20, format_header: Vec::new(), stream }
    }

    #[test]
    fn versions_4_and_5_round_trip() {
        let want = tone(Codec::Pcm16);
        assert_eq!(read(&write_v5(&want)).unwrap(), want);
        assert_eq!(read(&write_v4(&want)).unwrap(), want);
        let adpcm = Sound { codec: Codec::Adpcm, format_header: vec![2, 0, 2, 0, 1, 2, 3, 4], ..want };
        assert_eq!(read(&write_v4(&adpcm)).unwrap(), adpcm);
    }

    #[test]
    fn pcm_plays_as_riff_and_files_pass_through() {
        let (wav, ext) = tone(Codec::Pcm16).playable();
        assert_eq!((&wav[..4], &wav[8..16], ext), (&b"RIFF"[..], &b"WAVEfmt "[..], "wav"));
        assert_eq!((u32_at(&wav, 4), u32_at(&wav, 24), u32_at(&wav, 40)), (Some(wav.len() as u32 - 8), Some(22050), Some(128)));
        let ogg = Sound { stream: b"OggS rest".to_vec(), ..tone(Codec::Pcm16) };
        assert_eq!(ogg.playable(), (b"OggS rest".to_vec(), "ogg"));
        assert_eq!(Sound { codec: Codec::Mp3, ..tone(Codec::Mp3) }.playable().1, "mp3");
    }
}
