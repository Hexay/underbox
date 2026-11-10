//! `globalOVRLipSync`: the OVRLipSync C API managed's `SoundHandle.LipSync` calls each mix block,
//! answered by our estimator ([`audio::lipsync`]).

use abi::slots::global_ovr_lip_sync as ovr;
use abi::structs::Frame;
use abi::{RawPtr, Registry, bind};
use audio::lipsync::Context;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

/// `OVRLipSync.Result`.
const SUCCESS: i64 = 0;
const INVALID_PARAM: i64 = -2202;
/// OVR's default rate when a context doesn't name one.
const DEFAULT_RATE: u32 = 16000;

static CONTEXTS: LazyLock<Mutex<HashMap<u32, Context>>> = LazyLock::new(Mutex::default);
static NEXT: AtomicU32 = AtomicU32::new(1);

fn create(out: &mut u32, rate: u32) -> i64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    CONTEXTS.lock().unwrap().insert(id, Context::new(rate));
    *out = id;
    SUCCESS
}

/// `OVRLipSync.AudioDataType`: S16 mono, S16 stereo, F32 mono, F32 stereo.
fn mono(data: RawPtr<c_void>, frames: usize, kind: i64) -> Option<Vec<f32>> {
    let stereo = kind == 1 || kind == 3;
    let n = frames * if stereo { 2 } else { 1 };
    // SAFETY: managed passes `frames` frames of `kind` audio, valid for the call (SoundHandle.ProcessLipSync).
    let samples: Vec<f32> = match kind {
        0 | 1 => unsafe { data.cast::<i16>().as_slice(n) }.iter().map(|&s| s as f32 / 32768.0).collect(),
        2 | 3 => unsafe { data.cast::<f32>().as_slice(n) }.to_vec(),
        _ => return None,
    };
    Some(if stereo { samples.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5).collect() } else { samples })
}

fn process(ctx: u32, data: RawPtr<c_void>, frames: i32, kind: i64, frame: &mut Frame) -> i64 {
    let Some(samples) = mono(data, frames.max(0) as usize, kind) else { return INVALID_PARAM };
    let mut contexts = CONTEXTS.lock().unwrap();
    let Some(c) = contexts.get_mut(&ctx) else { return INVALID_PARAM };
    let weights = c.process(&samples);
    // SAFETY: `visemes` is managed's pinned float[visemes_length] (SoundHandle.ProcessLipSync).
    let out = unsafe { RawPtr::<f32>::from_raw(frame.visemes as *mut f32).as_mut_slice(frame.visemes_length as usize) };
    out.iter_mut().zip(weights).for_each(|(o, w)| *o = w);
    frame.frame_number = c.frame;
    frame.frame_delay = c.delay_ms(samples.len());
    frame.laughter_score = 0.0;
    SUCCESS
}

pub fn register(r: &mut Registry) {
    bind!(r, ovr::OVR_LIP_SYNC_INITIALIZE, |_, _| SUCCESS);
    bind!(r, ovr::OVR_LIP_SYNC_SHUTDOWN, || {
        CONTEXTS.lock().unwrap().clear();
        SUCCESS
    });
    bind!(r, ovr::OVR_LIP_SYNC_CREATE_CONTEXT, |out, _provider| create(out, DEFAULT_RATE));
    bind!(r, ovr::OVR_LIP_SYNC_CREATE_CONTEXT_EX, |out, _provider, rate: i32, _accel| create(out, rate.max(1) as u32));
    bind!(r, ovr::OVR_LIP_SYNC_DESTROY_CONTEXT, |ctx| if CONTEXTS.lock().unwrap().remove(&ctx).is_some() { SUCCESS } else { INVALID_PARAM });
    bind!(r, ovr::OVR_LIP_SYNC_RESET_CONTEXT, |ctx| CONTEXTS.lock().unwrap().get_mut(&ctx).map_or(INVALID_PARAM, |c| {
        c.reset();
        SUCCESS
    }));
    // Signals tune OVR's model (viseme on/off, smoothing); the estimator has nothing to tune.
    bind!(r, ovr::OVR_LIP_SYNC_SEND_SIGNAL, |ctx, _, _, _| if CONTEXTS.lock().unwrap().contains_key(&ctx) { SUCCESS } else { INVALID_PARAM });
    bind!(r, ovr::OVR_LIP_SYNC_PROCESS_FRAME_EX, process);
}
