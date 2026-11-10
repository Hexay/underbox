//! Decoded sounds: planar f32 at the sound's own rate, plus the loop region.

/// `SoundFormat` (Sandbox.Engine/Resources/Sound/SoundFile.cs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Format {
    Pcm16 = 0,
    Pcm8 = 1,
    Mp3 = 2,
    Adpcm = 3,
    Opus = 4,
    Vorbis = 5,
}

/// `CreateSound`'s format codes as managed sends them (SoundFile.Create): 3 carries f32 samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFormat {
    Pcm16,
    Pcm8,
    F32,
}

impl RawFormat {
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Pcm16),
            1 => Some(Self::Pcm8),
            3 => Some(Self::F32),
            _ => None,
        }
    }

    pub fn bytes(self) -> usize {
        match self {
            Self::Pcm16 => 2,
            Self::Pcm8 => 1,
            Self::F32 => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pcm {
    pub rate: u32,
    /// One Vec per channel, all `frames` long.
    pub channels: Vec<Vec<f32>>,
    /// Where a loop jumps back to; `None` plays once.
    pub loop_start: Option<usize>,
    /// Where a loop ends (exclusive); `None` is the end of the sound.
    pub loop_end: Option<usize>,
    /// What the sound was stored as, for `VSound_t.format`.
    pub format: Format,
    pub bits: u32,
}

impl Pcm {
    pub fn new(rate: u32, channels: Vec<Vec<f32>>) -> Self {
        Self { rate, channels, loop_start: None, loop_end: None, format: Format::Pcm16, bits: 16 }
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn duration(&self) -> f32 {
        if self.rate == 0 { 0.0 } else { self.frames() as f32 / self.rate as f32 }
    }

    /// The loop as `(start, end)` once both are resolved against the sound's length.
    pub fn loop_region(&self) -> Option<(usize, usize)> {
        let end = self.loop_end.filter(|&e| e > 0).unwrap_or(self.frames()).min(self.frames());
        self.loop_start.filter(|&s| s < end).map(|s| (s, end))
    }

    /// Interleaved little-endian samples as managed hands them to `CreateSound`.
    pub fn from_raw(format: RawFormat, channels: usize, rate: u32, frames: usize, data: &[u8]) -> Option<Self> {
        if channels == 0 {
            return None;
        }
        let frame_bytes = channels * format.bytes();
        let frames = frames.min(data.len() / frame_bytes);
        let mut out = vec![Vec::with_capacity(frames); channels];
        for frame in data[..frames * frame_bytes].chunks_exact(frame_bytes) {
            for (ch, s) in frame.chunks_exact(format.bytes()).enumerate() {
                out[ch].push(match format {
                    RawFormat::Pcm16 => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                    RawFormat::Pcm8 => (s[0] as f32 - 128.0) / 128.0,
                    RawFormat::F32 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                });
            }
        }
        let mut pcm = Self::new(rate, out);
        pcm.format = if format == RawFormat::Pcm8 { Format::Pcm8 } else { Format::Pcm16 };
        pcm.bits = 8 * format.bytes() as u32;
        Some(pcm)
    }

    /// The sound as i16: mono (channels averaged) or, with `interleaved` and two channels,
    /// L,R pairs. `CSfxTable_GetSamples*`.
    pub fn to_i16(&self, interleaved: bool) -> Vec<i16> {
        let n = self.channel_count();
        if interleaved && n == 2 {
            return (0..self.frames()).flat_map(|i| [to_i16(self.channels[0][i]), to_i16(self.channels[1][i])]).collect();
        }
        (0..self.frames()).map(|i| to_i16(self.channels.iter().map(|c| c[i]).sum::<f32>() / n.max(1) as f32)).collect()
    }

    /// Interleaved i16 little-endian bytes (`CSimplePCMWaveData`'s 16-bit output).
    pub fn to_pcm16_bytes(&self) -> Vec<u8> {
        (0..self.frames()).flat_map(|i| self.channels.iter().flat_map(move |c| to_i16(c[i]).to_le_bytes())).collect()
    }
}

pub fn to_i16(s: f32) -> i16 {
    (s * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_formats_decode_to_unit_floats() {
        let pcm16 = Pcm::from_raw(RawFormat::Pcm16, 2, 22050, 2, &[0x00, 0x40, 0x00, 0xC0, 0xFF, 0x7F, 0x00, 0x80]).unwrap();
        assert_eq!(pcm16.channels, vec![vec![0.5, 32767.0 / 32768.0], vec![-0.5, -1.0]]);
        let pcm8 = Pcm::from_raw(RawFormat::Pcm8, 1, 8000, 3, &[128, 192, 0]).unwrap();
        assert_eq!(pcm8.channels, vec![vec![0.0, 0.5, -1.0]]);
        assert_eq!((pcm8.format, pcm8.bits), (Format::Pcm8, 8));
        let f = Pcm::from_raw(RawFormat::F32, 1, 44100, 9, &0.25f32.to_le_bytes()).unwrap();
        assert_eq!(f.channels, vec![vec![0.25]], "frames are clamped to the data");
    }

    #[test]
    fn i16_export_downmixes_unless_stereo_interleaved() {
        let pcm = Pcm::new(44100, vec![vec![0.5, 0.0], vec![-0.5, 1.0]]);
        assert_eq!(pcm.to_i16(false), vec![0, 16384]);
        assert_eq!(pcm.to_i16(true), vec![16384, -16384, 0, 32767]);
    }

    #[test]
    fn loop_region_resolves_against_length() {
        let mut pcm = Pcm::new(44100, vec![vec![0.0; 100]]);
        assert_eq!(pcm.loop_region(), None);
        pcm.loop_start = Some(10);
        assert_eq!(pcm.loop_region(), Some((10, 100)));
        pcm.loop_end = Some(50);
        assert_eq!(pcm.loop_region(), Some((10, 50)));
        pcm.loop_start = Some(60);
        assert_eq!(pcm.loop_region(), None, "a start past the end never loops");
    }
}
