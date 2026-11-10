//! `CSimplePCMWaveData`: MP3 and OGG files decoded to 16-bit interleaved PCM for managed's
//! `SoundData.FromMP3`/`FromOGG`, which then builds a sound with `CreateSound`.

use abi::slots::c_simple_pcm_wave_data as w;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::ffi::c_void;
use std::sync::Mutex;

type Ref = NativeRef<class::CSimplePCMWaveData>;

/// `WAVE_FORMAT_PCM`.
const FORMAT_PCM: u32 = 1;

#[derive(Default)]
struct Wave {
    data: Box<[u8]>,
    format: u32,
    rate: u32,
    channels: u32,
    frames: u32,
    bits: u32,
    duration: f32,
}

#[derive(Default)]
struct WaveData(Mutex<Wave>);

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut Wave) -> R) -> R {
    objects::get::<_, WaveData>(r).map(|w| f(&mut w.0.lock().unwrap())).unwrap_or_default()
}

fn parse(r: Ref, data: RawPtr<c_void>, len: i32, ext: &str) -> bool {
    // SAFETY: managed pins the file's `len` bytes for the call (SoundData.FromMP3/FromOGG).
    let bytes = unsafe { data.cast::<u8>().as_slice(len.max(0) as usize) };
    let Ok(pcm) = audio::decode::decode(bytes, Some(ext)) else { return false };
    with(r, |w| {
        *w = Wave {
            data: pcm.to_pcm16_bytes().into(),
            format: FORMAT_PCM,
            rate: pcm.rate,
            channels: pcm.channel_count() as u32,
            frames: pcm.frames() as u32,
            bits: 16,
            duration: pcm.duration(),
        };
        true
    })
}

macro_rules! fields {
    ($r:expr, $($get:ident, $set:ident => $field:ident;)*) => {
        $(
            bind!($r, w::$get, |p: *mut c_void| with(Ref::from_raw(p), |w| w.$field));
            bind!($r, w::$set, |p: *mut c_void, v| with(Ref::from_raw(p), |w| w.$field = v));
        )*
    };
}

pub fn register(r: &mut Registry) {
    bind!(r, w::CREATE, || objects::create(WaveData::default()));
    bind!(r, w::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, w::PARSE_MP3_FILE, |r, data, len| {
        parse(r, data, len, "mp3");
    });
    bind!(r, w::PARSE_OGG_FILE, |r, data, len| parse(r, data, len, "ogg"));
    bind!(r, w::GET_PCM_DATA, |r| with(r, |w| RawPtr::from_raw(w.data.as_ptr() as *mut c_void)));
    bind!(r, w::GET_PCM_SIZE, |r| with(r, |w| w.data.len() as i32));
    fields! { r,
        GET_M_N_FORMAT, SET_M_N_FORMAT => format;
        GET_M_N_SAMPLE_RATE, SET_M_N_SAMPLE_RATE => rate;
        GET_M_N_CHANNELS, SET_M_N_CHANNELS => channels;
        GET_M_N_SAMPLE_COUNT, SET_M_N_SAMPLE_COUNT => frames;
        GET_M_N_BITS, SET_M_N_BITS => bits;
        GET_M_FL_DURATION, SET_M_FL_DURATION => duration;
    }
}
