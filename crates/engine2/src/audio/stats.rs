//! Counters that show whether a game's sounds reach the mix: voices started, the blocks samplers read
//! and the blocks sent to the device, each split by whether they carried any signal.
//! `UNDERBOX_AUDIO_STATS=1` logs them every `LOG_EVERY` output blocks (tools/game.sh MODE=sound reads it).

use audio::Block;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// About 5 s of 512-frame blocks at 44.1 kHz.
const LOG_EVERY: u64 = 430;

pub struct Stats {
    pub voices: AtomicU64,
    pub voice_failures: AtomicU64,
    pub voice_blocks: AtomicU64,
    pub audible_voice_blocks: AtomicU64,
    pub output_blocks: AtomicU64,
    pub audible_output_blocks: AtomicU64,
    /// Loudest output sample so far, as f32 bits.
    peak: AtomicU32,
}

pub static STATS: Stats = Stats {
    voices: AtomicU64::new(0),
    voice_failures: AtomicU64::new(0),
    voice_blocks: AtomicU64::new(0),
    audible_voice_blocks: AtomicU64::new(0),
    output_blocks: AtomicU64::new(0),
    audible_output_blocks: AtomicU64::new(0),
    peak: AtomicU32::new(0),
};

static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var("UNDERBOX_AUDIO_STATS").is_ok_and(|v| v == "1"));

fn peak_of(blocks: &[Block]) -> f32 {
    blocks.iter().flat_map(|b| b.iter()).fold(0.0, |m, s| m.max(s.abs()))
}

pub fn voice_started(ok: bool) {
    if ok { &STATS.voices } else { &STATS.voice_failures }.fetch_add(1, Relaxed);
}

pub fn voice_read(blocks: &[Block]) {
    STATS.voice_blocks.fetch_add(1, Relaxed);
    if peak_of(blocks) > 0.0 {
        STATS.audible_voice_blocks.fetch_add(1, Relaxed);
    }
}

pub fn output_sent(blocks: &[Block]) {
    let n = STATS.output_blocks.fetch_add(1, Relaxed) + 1;
    let peak = peak_of(blocks);
    if peak > 0.0 {
        STATS.audible_output_blocks.fetch_add(1, Relaxed);
        STATS.peak.fetch_max(peak.to_bits(), Relaxed);
    }
    if *ENABLED && n.is_multiple_of(LOG_EVERY) {
        abi::rt::log::log(abi::rt::log::Level::Info, &summary());
    }
}

#[cfg(test)]
#[test]
fn silent_and_audible_blocks_are_told_apart() {
    let audible = STATS.audible_voice_blocks.load(Relaxed);
    let mut loud = audio::blocks(2);
    loud[1][7] = -0.5;
    voice_read(&audio::blocks(2));
    voice_read(&loud);
    // Other tests may mix concurrently, so only a lower bound holds.
    assert!(STATS.audible_voice_blocks.load(Relaxed) > audible);
    output_sent(&loud);
    assert!(summary().contains("peak=0.5") || f32::from_bits(STATS.peak.load(Relaxed)) >= 0.5);
}

pub fn summary() -> String {
    let get = |c: &AtomicU64| c.load(Relaxed);
    let s = &STATS;
    format!(
        "audio stats: voices={} voice_failures={} voice_blocks={} audible_voice_blocks={} output_blocks={} audible_output_blocks={} peak={:.3}",
        get(&s.voices),
        get(&s.voice_failures),
        get(&s.voice_blocks),
        get(&s.audible_voice_blocks),
        get(&s.output_blocks),
        get(&s.audible_output_blocks),
        // Non-negative floats order like their bits.
        f32::from_bits(s.peak.load(Relaxed)),
    )
}
