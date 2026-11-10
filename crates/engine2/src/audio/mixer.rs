//! `CAudioMixer`: one playing voice's sampler (managed's `AudioSampler`). The mix thread reads it
//! each block; the main thread seeks it, so it sits behind a mutex.

use super::mix::{self, DevRef};
use super::sound::SfxRef;
use abi::slots::c_audio_mixer as m;
use abi::structs::audio_source_indexstate_t as IndexState;
use abi::{NativeRef, Registry, bind, class};
use audio::sampler::Sampler;
use std::sync::Mutex;

type Ref = NativeRef<class::CAudioMixer>;

struct Mixer {
    sampler: Mutex<Sampler>,
    table: SfxRef,
}

pub fn create(sampler: Sampler, table: SfxRef) -> Ref {
    objects::create(Mixer { sampler: Mutex::new(sampler), table })
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut Sampler) -> R) -> R {
    objects::get::<_, Mixer>(r).map(|m| f(&mut m.sampler.lock().unwrap())).unwrap_or_default()
}

/// `m_nSampleFracOffset` holds the fraction as 32-bit fixed point.
const FRAC_ONE: f64 = 4_294_967_296.0;

fn index_state(s: &Sampler) -> IndexState {
    IndexState {
        m_n_packet_index: 0,
        m_n_buffer_sample_offset: s.position() as u32,
        m_n_sample_frac_offset: (s.position_frac() * FRAC_ONE) as u32,
        m_n_delay_samples: s.delay(),
    }
}

fn read_to_buffer(r: Ref, pitch: f32, out: DevRef) {
    let mut blocks = audio::blocks(mix::channel_count(out));
    with(r, |s| s.read(pitch, &mut blocks));
    mix::write_all(out, &blocks);
}

pub fn register(r: &mut Registry) {
    bind!(r, m::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, m::GET_SAMPLE_POSITION, |r| with(r, |s| s.position() as i32));
    bind!(r, m::SET_SAMPLE_POSITION, |r, p: i32| with(r, |s| s.set_position(p.max(0) as u64, 0.0)));
    bind!(r, m::SHOULD_CONTINUE_MIXING, |r| with(r, |s| s.should_continue()));
    bind!(r, m::SET_SAMPLE_END, |r, end: u32| with(r, |s| s.set_sample_end(end as usize)));
    bind!(r, m::DELAY_OR_SKIP_SAMPLES, |r, n| with(r, |s| s.delay_or_skip(n)));
    bind!(r, m::IS_READY_TO_MIX, |r| objects::get::<_, Mixer>(r).is_some());
    bind!(r, m::GET_POSITION_FOR_SAVE, |r| with(r, |s| s.position() as i32));
    bind!(r, m::SET_POSITION_FROM_SAVED, |r, p: i32| with(r, |s| s.set_position(p.max(0) as u64, 0.0)));
    bind!(r, m::UPDATE_MIXER_STATE, |r, st: IndexState| {
        with(r, |s| s.set_position(st.m_n_buffer_sample_offset as u64, st.m_n_sample_frac_offset as f64 / FRAC_ONE))
    });
    bind!(r, m::GET_INDEX_STATE, |r| objects::get::<_, Mixer>(r).map_or_else(IndexState::default, |m| index_state(&m.sampler.lock().unwrap())));
    bind!(r, m::GET_SFX_TABLE, |r| objects::get::<_, Mixer>(r).map_or(SfxRef::null(), |m| m.table));
    bind!(r, m::GET_SAMPLE_COUNT, |r| with(r, |s| s.sample_count() as i32));
    bind!(r, m::GET_CHANNEL_COUNT, |r| with(r, |s| s.channels() as i32));
    bind!(r, m::SET_TIME_SCALE, |r, scale| with(r, |s| s.set_time_scale(scale)));
    bind!(r, m::ENABLE_LOOPING, |r, on| with(r, |s| s.set_looping(on)));
    bind!(r, m::READ_TO_BUFFER, read_to_buffer);
}
