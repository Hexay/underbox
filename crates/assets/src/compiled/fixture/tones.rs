//! Synthetic sounds for corpus/sound (the audio differential, docs/differential-audio.md): mono
//! PCM16 at 44100 Hz, so neither engine resamples them at pitch 1.

use super::super::sound::{Codec, Sound, write_v4};

pub const TONE_RATE: u32 = 44100;
/// Peak amplitude of every fixture sound, as a fraction of full scale.
pub const TONE_LEVEL: f32 = 0.5;
/// Fade in and out, so onsets and stops don't click.
const RAMP_S: f32 = 0.005;

/// (compiled path, frequency in Hz or 0 for noise, seconds, loops).
pub const TONES: [(&str, f32, f32, bool); 3] = [
    ("sounds/compiled/sine440.vsnd", 440.0, 0.5, false),
    ("sounds/compiled/noise.vsnd", 0.0, 0.3, false),
    // A whole number of cycles, so the loop seam is continuous.
    ("sounds/compiled/loop330.vsnd", 330.0, 1.0, true),
];

/// A fixed LCG: the same noise on every machine.
fn noise(n: usize) -> impl Iterator<Item = f32> {
    let mut x: u32 = 0x1234_5678;
    (0..n).map(move |_| {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (x >> 8) as f32 / (1 << 23) as f32 - 1.0
    })
}

pub fn tone(freq: f32, seconds: f32, looping: bool) -> Sound {
    let n = (seconds * TONE_RATE as f32).round() as usize;
    let ramp = (RAMP_S * TONE_RATE as f32) as usize;
    let wave: Vec<f32> = if freq > 0.0 {
        (0..n).map(|i| (std::f32::consts::TAU * freq * i as f32 / TONE_RATE as f32).sin()).collect()
    } else {
        noise(n).collect()
    };
    let stream = wave
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let edge = if looping { 1.0 } else { (i.min(n - 1 - i) as f32 / ramp as f32).min(1.0) };
            (v * edge * TONE_LEVEL * i16::MAX as f32).round() as i16
        })
        .flat_map(i16::to_le_bytes)
        .collect();
    Sound {
        codec: Codec::Pcm16,
        rate: TONE_RATE,
        channels: 1,
        sample_count: n as u32,
        duration: seconds,
        loop_start: if looping { 0 } else { -1 },
        loop_end: 0,
        format_header: Vec::new(),
        stream,
    }
}

/// (compiled path, bytes) of every fixture sound.
pub fn tone_files() -> Vec<(String, Vec<u8>)> {
    TONES.iter().map(|&(path, freq, secs, looping)| (format!("{path}_c"), write_v4(&tone(freq, secs, looping)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// corpus/sound ships these files; `UNDERBOX_BLESS=1` rewrites them after a change here.
    #[test]
    fn corpus_sounds_match_the_fixture() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/sound/Assets");
        for (path, want) in tone_files() {
            let file = assets.join(&path);
            if std::env::var("UNDERBOX_BLESS").as_deref() == Ok("1") {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, &want).unwrap();
            }
            assert!(std::fs::read(&file).ok() == Some(want), "{} is stale; rerun with UNDERBOX_BLESS=1", file.display());
        }
    }

    #[test]
    fn tones_round_trip_with_their_loop() {
        let s = super::super::super::sound::read(&write_v4(&tone(330.0, 1.0, true))).unwrap();
        assert_eq!((s.rate, s.channels, s.sample_count, s.loop_start, s.stream.len()), (44100, 1, 44100, 0, 88200));
    }
}
