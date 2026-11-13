//! `CAudioStreamManaged` (managed `SoundStream`): a `[Handle]` object over an [`audio::stream::Stream`].
//! Its table's mixers hold the stream too, so `Destroy` never frees it under a playing voice.

use super::sound::{Sfx, SfxRef};
use crate::handles::{Handle, Kind};
use abi::slots::c_audio_stream_managed as s;
use abi::{HandleId, NativeRef, RawPtr, Registry, StrRet, bind, class};
use audio::sampler::Source;
use audio::stream::Stream;
use std::ffi::c_void;
use std::sync::Arc;

pub(crate) type Ref = NativeRef<class::CAudioStreamManaged>;

struct Managed {
    stream: Arc<Stream>,
    table: SfxRef,
    handle: Handle,
}

const NAME: &str = "audiostream";

fn create(channels: i32, rate: u32) -> HandleId<class::CAudioStreamManaged> {
    adopt(Arc::new(Stream::new(channels.clamp(1, 16) as usize, rate))).1
}

/// A managed handle over a stream native already writes (a video's audio track).
pub(crate) fn adopt(stream: Arc<Stream>) -> (Ref, HandleId<class::CAudioStreamManaged>) {
    let table = objects::create_shared(Sfx::ready(NAME, Source::Stream(stream.clone())));
    let r: Ref = objects::create(Managed { stream, table, handle: Handle::default() });
    (r, objects::get::<_, Managed>(r).map_or(HandleId::new(-1), |m| m.handle.register(r, Kind::AudioStream)))
}

/// Free the handle and the object (`Destroy`); voices already playing keep the stream.
pub(crate) fn destroy(r: Ref) {
    if let Some(m) = objects::get::<_, Managed>(r) {
        m.handle.free();
        objects::destroy(m.table);
    }
    objects::destroy(r);
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&Stream) -> R) -> R {
    objects::get::<_, Managed>(r).map(|m| f(&m.stream)).unwrap_or_default()
}

fn write(r: Ref, data: RawPtr<c_void>, frames: u32, channels: u32) {
    // SAFETY: managed pins `frames * channels` shorts for the call (SoundStream.WriteData).
    let samples = unsafe { data.cast::<i16>().as_slice(frames as usize * channels as usize) };
    with(r, |s| {
        s.write(samples, frames as usize, channels as usize);
    });
}

pub fn register(r: &mut Registry) {
    bind!(r, s::CREATE, create);
    bind!(r, s::DESTROY, destroy);
    bind!(r, s::WRITE_AUDIO_DATA, write);
    bind!(r, s::QUEUED_SAMPLE_COUNT, |r| with(r, |s| s.queued_frames() as u32));
    bind!(r, s::MAX_WRITE_SAMPLE_COUNT, |r| with(r, |s| s.free_frames() as u32));
    // What's queued plus the block a mixer holds, in stream frames.
    bind!(r, s::LATENCY_SAMPLES_COUNT, |r| with(r, |s| (s.queued_frames() + audio::BLOCK * s.rate as usize / audio::RATE as usize) as u32));
    bind!(r, s::PAUSE, |r| with(r, |s| s.set_paused(true)));
    bind!(r, s::RESUME, |r| with(r, |s| s.set_paused(false)));
    bind!(r, s::CLOSE, |r| with(r, Stream::close));
    bind!(r, s::GET_NAME, |r| StrRet::from(objects::get::<_, Managed>(r).map(|_| NAME)));
    bind!(r, s::GET_SFX_TABLE, |r| objects::get::<_, Managed>(r).map_or(SfxRef::null(), |m| m.table));
}
