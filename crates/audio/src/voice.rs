//! Voice chat's codec: libopus, mono, as `VoiceManager` drives it (48 kHz, 20 ms frames,
//! 32 kbit/s). Real libopus keeps packets compatible with s&box clients.

use std::sync::Mutex;

pub struct Encoder(Mutex<opus::Encoder>);

impl Encoder {
    pub fn new(rate: u32, bitrate: i32) -> Option<Self> {
        let mut e = opus::Encoder::new(rate, opus::Channels::Mono, opus::Application::Voip).ok()?;
        if bitrate > 0 {
            e.set_bitrate(opus::Bitrate::Bits(bitrate)).ok()?;
        }
        Some(Self(Mutex::new(e)))
    }

    /// One frame of f32 samples into `out`; the packet's length, or 0 on failure.
    pub fn encode(&self, pcm: &[f32], out: &mut [u8]) -> usize {
        self.0.lock().unwrap().encode_float(pcm, out).unwrap_or(0)
    }
}

pub struct Decoder(Mutex<opus::Decoder>);

impl Decoder {
    pub fn new(rate: u32) -> Option<Self> {
        opus::Decoder::new(rate, opus::Channels::Mono).ok().map(|d| Self(Mutex::new(d)))
    }

    /// One packet into `out`; the samples decoded, or 0 on failure.
    pub fn decode(&self, packet: &[u8], out: &mut [i16]) -> usize {
        self.0.lock().unwrap().decode(packet, out, false).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;
    const FRAME: usize = RATE as usize / 50;

    #[test]
    fn a_tone_survives_the_round_trip() {
        let enc = Encoder::new(RATE, 32000).unwrap();
        let dec = Decoder::new(RATE).unwrap();
        let mut decoded = Vec::new();
        for f in 0..10 {
            let pcm: Vec<f32> = (0..FRAME).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * (f * FRAME + i) as f32 / RATE as f32).sin()).collect();
            let mut packet = [0u8; 4000];
            let n = enc.encode(&pcm, &mut packet);
            assert!(n > 0 && n < 400, "a 20 ms packet at 32 kbit/s is about 80 bytes, got {n}");
            let mut out = vec![0i16; RATE as usize * 120 / 1000];
            let samples = dec.decode(&packet[..n], &mut out);
            assert_eq!(samples, FRAME);
            decoded.extend_from_slice(&out[..samples]);
        }
        let tail = &decoded[decoded.len() / 2..];
        let rms = (tail.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / tail.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.08, "rms {rms}");
    }

    #[test]
    fn bad_input_fails_softly() {
        assert!(Encoder::new(44100, 32000).is_none(), "opus has no 44.1 kHz mode");
        let dec = Decoder::new(RATE).unwrap();
        let mut out = [0i16; 10];
        assert_eq!(dec.decode(&[0xFF; 3], &mut out), 0);
    }
}
