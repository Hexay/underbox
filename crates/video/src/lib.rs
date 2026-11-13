//! Video playback and recording for engine2's `CVideoPlayer`/`CVideoRecorder` (docs/video.md):
//! containers ([`demux`]), decoders ([`decode`]), the playback worker ([`player`]) and the
//! H.264/MP4 writer ([`recorder`]). Knows nothing about slots, textures or mixers; engine2 wires
//! those in through [`player::Events`] and [`player::AudioSink`].

pub mod decode;
pub mod demux;
pub mod encode;
pub mod fixture;
pub mod player;
pub mod recorder;
mod spectrum;
#[cfg(test)]
mod tests;
mod yuv;

pub use player::{AudioSink, Events, Frame, Info, Options, Player, Stats};
