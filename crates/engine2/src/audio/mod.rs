//! Audio slots over the `audio` crate. Managed mixes; native loads sounds, samples voices, runs
//! effects and DSP, codes voice and feeds the device. See docs/audio.md.

pub mod device;
mod dsp;
mod effects;
mod lipsync;
pub mod mix;
mod mixer;
mod pcm_wave;
pub mod sound;
mod stream;
mod voice;

#[cfg(test)]
mod tests;

use abi::Registry;

pub fn register(r: &mut Registry) {
    mix::register(r);
    device::register(r);
    sound::register(r);
    mixer::register(r);
    stream::register(r);
    pcm_wave::register(r);
    effects::register(r);
    dsp::register(r);
    voice::register(r);
    lipsync::register(r);
}
