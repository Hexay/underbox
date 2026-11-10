//! Audio slot contracts, called through the installed thunk table the way managed calls them.

use super::mix::{self, DevRef};
use abi::slots::{
    c_audio_mix_device_buffers as dev, c_audio_mixer as m, c_audio_processor as proc_, c_binaural_effect as bin,
    c_direct_effect as direct, c_sfx_table as sfx, c_simple_pcm_wave_data as wave, dsp_instance as inst, dsp_preset as dsp, g_p_sound_system_internal as system,
    global_ovr_lip_sync as ovr, v_sound_t as vs, voice_glue as voice,
};
use abi::structs::{Frame, Vector3};
use abi::{NativeRef, StringToken};
use audio::{BLOCK, Block};
use std::ffi::{c_char, c_void};

type P = *mut c_void;

macro_rules! slot {
    ($m:ident::$slot:ident: $ty:ty) => {
        thunk::<$ty>($m::$slot.index)
    };
}

fn thunk<F: Copy>(index: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    // SAFETY: F is the thunk's own extern "C" signature, spelled out at each use.
    unsafe { std::mem::transmute_copy(&crate::igen::test_table()[index]) }
}

fn buffers(channels: i32) -> DevRef {
    DevRef::from_raw(unsafe { slot!(dev::CREATE: unsafe extern "C" fn(i32) -> P)(channels) })
}

/// A sound from interleaved PCM16 through `CreateSound`, as `SoundFile.FromPcm` makes one.
fn pcm16_sound(rate: i32, channels: i32, samples: &[i16], loop_start: i32) -> P {
    let create = slot!(system::CREATE_SOUND: unsafe extern "C" fn(*const c_char, i32, i32, i32, i32, f32, i32, i32, P, i32) -> P);
    let mut bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let frames = samples.len() as i32 / channels;
    unsafe { create(c"test.vsnd".as_ptr(), channels, rate, 0, frames, 0.0, loop_start, 0, bytes.as_mut_ptr().cast(), bytes.len() as i32) }
}

fn mixer(table: P) -> P {
    unsafe { slot!(sfx::CREATE_MIXER: unsafe extern "C" fn(P) -> P)(table) }
}

fn read_to_buffer(mixer: P, pitch: f32, out: DevRef) {
    unsafe { slot!(m::READ_TO_BUFFER: unsafe extern "C" fn(P, f32, P))(mixer, pitch, out.as_raw()) }
}

#[test]
fn a_created_sound_plays_through_a_mixer_into_mix_buffers() {
    let table = pcm16_sound(22050, 2, &[16384, -16384, 0, 0, 16384, -16384, 0, 0], -1);
    assert!(unsafe { slot!(sfx::IS_VALID_FOR_PLAYBACK: unsafe extern "C" fn(P) -> i32)(table) } != 0);
    let mx = mixer(table);
    assert_eq!(unsafe { slot!(m::GET_CHANNEL_COUNT: unsafe extern "C" fn(P) -> i32)(mx) }, 2);
    assert_eq!(unsafe { slot!(m::GET_SAMPLE_COUNT: unsafe extern "C" fn(P) -> i32)(mx) }, 4);
    assert_eq!(unsafe { slot!(m::GET_SFX_TABLE: unsafe extern "C" fn(P) -> P)(mx) }, table);

    let out = buffers(2);
    read_to_buffer(mx, 1.0, out);
    let [l, r]: [Block; 2] = mix::read_all(out).try_into().unwrap();
    assert_eq!(&l[..8], &[0.5, 0.25, 0.0, 0.25, 0.5, 0.25, 0.0, 0.0], "22.05 kHz doubled to 44.1 by interpolation");
    assert_eq!(&r[..4], &[-0.5, -0.25, 0.0, -0.25]);
    assert_eq!(unsafe { slot!(m::SHOULD_CONTINUE_MIXING: unsafe extern "C" fn(P) -> i32)(mx) }, 0, "a one-shot ends");

    unsafe { slot!(m::SET_SAMPLE_POSITION: unsafe extern "C" fn(P, i32))(mx, 2) };
    assert_eq!(unsafe { slot!(m::GET_SAMPLE_POSITION: unsafe extern "C" fn(P) -> i32)(mx) }, 2);
    assert_ne!(unsafe { slot!(m::SHOULD_CONTINUE_MIXING: unsafe extern "C" fn(P) -> i32)(mx) }, 0, "seeking back revives it");
    unsafe { slot!(m::DISPOSE: unsafe extern "C" fn(P))(mx) };
}

#[test]
fn a_looping_sound_keeps_mixing() {
    let mx = mixer(pcm16_sound(44100, 1, &[16384, -16384], 0));
    let out = buffers(1);
    read_to_buffer(mx, 1.0, out);
    let b = mix::read_all(out)[0];
    assert!(b.chunks(2).all(|p| p == [0.5, -0.5]));
    assert_ne!(unsafe { slot!(m::SHOULD_CONTINUE_MIXING: unsafe extern "C" fn(P) -> i32)(mx) }, 0);
}

#[test]
fn vsound_handles_describe_the_sound() {
    let table = pcm16_sound(22050, 2, &[100; 2 * 2205], -1);
    let snd = unsafe { slot!(sfx::GET_SOUND: unsafe extern "C" fn(P) -> P)(table) };
    assert_eq!(unsafe { slot!(vs::M_RATE: unsafe extern "C" fn(P) -> i32)(snd) }, 22050);
    assert_eq!(unsafe { slot!(vs::CHANNELS: unsafe extern "C" fn(P) -> i32)(snd) }, 2);
    assert_eq!(unsafe { slot!(vs::M_SAMPLE_FRAME_SIZE: unsafe extern "C" fn(P) -> i32)(snd) }, 4);
    assert!((unsafe { slot!(vs::DURATION: unsafe extern "C" fn(P) -> f32)(snd) } - 0.1).abs() < 1e-6);
    let copy = unsafe { slot!(vs::COPY_STRONG_HANDLE: unsafe extern "C" fn(P) -> P)(snd) };
    unsafe { slot!(vs::DESTROY_STRONG_HANDLE: unsafe extern "C" fn(P))(snd) };
    let valid = slot!(vs::IS_STRONG_HANDLE_VALID: unsafe extern "C" fn(P) -> i32);
    assert_eq!(unsafe { (valid(snd), valid(copy)) }, (0, 1));

    let mut samples = vec![0i16; 2205];
    assert!(unsafe { slot!(sfx::GET_SAMPLES: unsafe extern "C" fn(P, P, u32) -> i32)(table, samples.as_mut_ptr().cast(), 2205) } != 0);
    assert!(samples.iter().all(|&s| s == 100));
}

#[test]
fn a_missing_sound_fails_to_load_instead_of_crashing() {
    let table = unsafe { slot!(system::PRECACHE_SOUND: unsafe extern "C" fn(*const c_char) -> P)(c"sounds/nowhere.vsnd".as_ptr()) };
    assert!(!table.is_null());
    assert!(mixer(table).is_null());
    assert_ne!(unsafe { slot!(sfx::FAILED_RESOURCE_LOAD: unsafe extern "C" fn(P) -> i32)(table) }, 0);
    assert_eq!(unsafe { slot!(sfx::GET_CACHE_STATUS: unsafe extern "C" fn(P) -> i32)(table) }, 2);
}

#[test]
fn ogg_parses_to_pcm16_for_sound_data() {
    let w = unsafe { slot!(wave::CREATE: unsafe extern "C" fn() -> P)() };
    let mut ogg = include_bytes!("../../../audio/testdata/sine441.ogg").to_vec();
    assert!(unsafe { slot!(wave::PARSE_OGG_FILE: unsafe extern "C" fn(P, P, i32) -> i32)(w, ogg.as_mut_ptr().cast(), ogg.len() as i32) } != 0);
    let get = |slot: usize| unsafe { thunk::<unsafe extern "C" fn(P) -> u32>(slot)(w) };
    assert_eq!((get(wave::GET_M_N_SAMPLE_RATE.index), get(wave::GET_M_N_CHANNELS.index), get(wave::GET_M_N_BITS.index)), (44100, 1, 16));
    let frames = get(wave::GET_M_N_SAMPLE_COUNT.index);
    assert_eq!(unsafe { slot!(wave::GET_PCM_SIZE: unsafe extern "C" fn(P) -> i32)(w) }, frames as i32 * 2);
}

#[test]
fn streams_feed_their_mixers_and_close() {
    // `CdStrmMngd_Create` registers a managed handle, so build its table the way it does.
    let stream = std::sync::Arc::new(audio::stream::Stream::new(1, 44100));
    let table: NativeRef<abi::class::CSfxTable> = objects::create_shared(super::sound::Sfx::ready("s", audio::sampler::Source::Stream(stream.clone())));
    stream.write(&[16384; 600], 600, 1);
    let mx = mixer(table.as_raw());
    let out = buffers(1);
    read_to_buffer(mx, 1.0, out);
    assert!(mix::read_all(out)[0].iter().all(|&v| v == 0.5));
    stream.close();
    read_to_buffer(mx, 1.0, out);
    assert_eq!(unsafe { slot!(m::SHOULD_CONTINUE_MIXING: unsafe extern "C" fn(P) -> i32)(mx) }, 0);
}

#[test]
fn effects_apply_between_buffer_sets() {
    let (input, output) = (buffers(1), buffers(2));
    mix::write_all(input, &[[1.0; BLOCK]]);

    let d = unsafe { slot!(direct::CREATE: unsafe extern "C" fn() -> P)() };
    let apply = slot!(direct::APPLY: unsafe extern "C" fn(P, f32, f32, f32, f32, f32, f32, f32, f32, P, P));
    unsafe { apply(d, 0.25, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, input.as_raw(), output.as_raw()) };
    assert!(mix::read_all(output)[0].iter().all(|&v| (v - 0.25).abs() < 1e-5), "distance attenuation scales the input");

    let b = unsafe { slot!(bin::CREATE: unsafe extern "C" fn() -> P)() };
    let apply = slot!(bin::APPLY: unsafe extern "C" fn(P, *const Vector3, f32, i32, P, P));
    let left = crate::math::vec3(0.0, 1.0, 0.0);
    unsafe { apply(b, &left, 1.0, 1, input.as_raw(), output.as_raw()) };
    let [l, r]: [Block; 2] = mix::read_all(output).try_into().unwrap();
    assert!(l[0] > 0.09 && r[0] < 0.001, "a source on the left: {} / {}", l[0], r[0]);

    let p = unsafe { slot!(proc_::CREATE_DELAY: unsafe extern "C" fn(i32) -> P)(1) };
    let set = slot!(proc_::SET_CONTROL_PARAMETER: unsafe extern "C" fn(P, StringToken, f32) -> i32);
    assert!(unsafe { set(p, StringToken::of("gain"), 0.0) } != 0);
    assert_eq!(unsafe { set(p, StringToken::of("nonsense"), 0.0) }, 0);
}

#[test]
fn dsp_presets_instantiate_per_channel() {
    let preset = unsafe { slot!(dsp::CREATE: unsafe extern "C" fn(*const c_char) -> P)(c"test.delay".as_ptr()) };
    let mut params = [0.0f32, 1000.0 / 44100.0 * 4.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    unsafe { slot!(dsp::ADD_PROCESSOR: unsafe extern "C" fn(P, i32, *mut f32, u32))(preset, 1, params.as_mut_ptr(), params.len() as u32) };
    let i = unsafe { slot!(dsp::INSTANTIATE: unsafe extern "C" fn(P, i32) -> P)(preset, 2) };
    let (src, dst) = (buffers(1), buffers(1));
    let mut impulse = audio::SILENT;
    impulse[0] = 1.0;
    mix::write_all(src, &[impulse]);
    let process = slot!(inst::PROCESS_CHANNEL: unsafe extern "C" fn(P, P, P, i32));
    let (bs, bd) = unsafe { (slot!(dev::GET_BUFFER: unsafe extern "C" fn(P, i32) -> P)(src.as_raw(), 0), slot!(dev::GET_BUFFER: unsafe extern "C" fn(P, i32) -> P)(dst.as_raw(), 0)) };
    unsafe { process(i, bs, bd, 0) };
    let out = mix::read_all(dst)[0];
    assert_eq!((out[0], out[4]), (1.0, 0.5));
}

#[test]
fn voice_codec_round_trips_through_the_glue() {
    let enc = unsafe { slot!(voice::CREATE_ENCODER: unsafe extern "C" fn(i32, i32) -> P)(48000, 32000) };
    let dec = unsafe { slot!(voice::CREATE_DECODER: unsafe extern "C" fn(i32) -> P)(48000) };
    assert!(!enc.is_null() && !dec.is_null());
    let mut pcm: Vec<f32> = (0..960).map(|i| (i as f32 * 0.06).sin() * 0.3).collect();
    let mut packet = [0u8; 1024];
    let n = unsafe { slot!(voice::ENCODE: unsafe extern "C" fn(P, P, i32, P, i32) -> i32)(enc, pcm.as_mut_ptr().cast(), 960, packet.as_mut_ptr().cast(), 1024) };
    assert!(n > 0);
    let mut out = [0i16; 5760];
    let samples = unsafe { slot!(voice::DECODE: unsafe extern "C" fn(P, P, i32, P, i32) -> i32)(dec, packet.as_mut_ptr().cast(), n, out.as_mut_ptr().cast(), 5760) };
    assert_eq!(samples, 960);
}

#[test]
fn lip_sync_writes_visemes_into_managed_frames() {
    let mut ctx = 0u32;
    assert_eq!(unsafe { slot!(ovr::OVR_LIP_SYNC_CREATE_CONTEXT_EX: unsafe extern "C" fn(*mut u32, i64, i32, i32) -> i64)(&mut ctx, 2, 48000, 1) }, 0);
    let mut visemes = [9.0f32; 15];
    let mut frame = Frame::default();
    frame.visemes = visemes.as_mut_ptr() as usize;
    frame.visemes_length = 15;
    let mut silence = [0.0f32; BLOCK];
    let r = unsafe { slot!(ovr::OVR_LIP_SYNC_PROCESS_FRAME_EX: unsafe extern "C" fn(u32, P, i32, i64, *mut Frame) -> i64)(ctx, silence.as_mut_ptr().cast(), BLOCK as i32, 2, &mut frame) };
    assert_eq!((r, frame.frame_number), (0, 1));
    assert_eq!(visemes[0], 1.0, "silence is sil");
    assert!(visemes[1..].iter().all(|&v| v == 0.0));
}
