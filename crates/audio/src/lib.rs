//! soundsystem. Managed (Sandbox.Engine/Systems/Audio) owns the mixer graph; this crate decodes
//! sounds, samples voices into fixed-size blocks, runs the native effects and DSP presets, codes
//! voice chat, and queues the mixed output for a device. See docs/audio.md.

pub mod convert;
pub mod decode;
#[cfg(feature = "device")]
pub mod device;
pub mod dsp;
pub mod effects;
pub mod lipsync;
pub mod output;
pub mod pcm;
pub mod sampler;
pub mod stream;
pub mod voice;

/// `MIX_BUFFER_SIZE`: samples per channel in one mix block.
pub const BLOCK: usize = 512;
/// `MIX_DEFAULT_SAMPLING_RATE`: the rate managed mixes at.
pub const RATE: u32 = 44100;
/// Managed's `MultiChannelBuffer` holds at most this many channels.
pub const MAX_CHANNELS: usize = 8;

/// One channel of one mix block.
pub type Block = [f32; BLOCK];

pub const SILENT: Block = [0.0; BLOCK];

/// `channels` silent blocks.
pub fn blocks(channels: usize) -> Vec<Block> {
    vec![SILENT; channels]
}
