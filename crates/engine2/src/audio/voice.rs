//! `VoiceGlue`: microphone capture and the Opus codec behind managed's `VoiceManager`. Its
//! handles are bare pointers on the managed side; ours are object addresses.

use abi::slots::voice_glue as v;
use abi::{NativeClass, NativeRef, RawPtr, Registry, StrRet, bind};
use audio::voice::{Decoder, Encoder};
use std::ffi::{CStr, c_void};

/// The class voice objects are made as (`VoiceGlue` has no native class of its own).
struct Voice;

impl NativeClass for Voice {
    const NAME: &'static str = "VoiceGlue";
}

fn ptr<T: std::any::Any + Send + Sync>(value: Option<T>) -> RawPtr<c_void> {
    value.map_or(RawPtr::NULL, |v| RawPtr::from_raw(objects::create::<Voice, _>(v).as_raw()))
}

fn get<T: std::any::Any + Send + Sync>(p: RawPtr<c_void>) -> Option<std::sync::Arc<T>> {
    objects::get::<Voice, T>(NativeRef::from_raw(p.as_ptr()))
}

fn destroy(p: RawPtr<c_void>) {
    objects::destroy(NativeRef::<Voice>::from_raw(p.as_ptr()));
}

#[cfg(feature = "audio-device")]
mod capture {
    use super::super::device::{Mode, mode};
    pub use audio::device::Capture;

    pub fn inputs() -> Vec<audio::DeviceInfo> {
        if mode() == Mode::Real { audio::device::inputs() } else { Vec::new() }
    }

    pub fn open(name: &str, rate: u32) -> Option<Capture> {
        if mode() != Mode::Real {
            return None;
        }
        audio::device::open_capture(Some(name), rate).inspect_err(|e| abi::rt::log::warn(&format!("voice capture: {e}"))).ok()
    }
}

#[cfg(not(feature = "audio-device"))]
mod capture {
    pub struct Capture;

    impl Capture {
        pub fn read(&self, _: &mut [f32]) -> usize {
            0
        }
    }

    pub fn inputs() -> Vec<audio::DeviceInfo> {
        Vec::new()
    }

    pub fn open(_: &str, _: u32) -> Option<Capture> {
        None
    }
}

use capture::Capture;

pub fn register(r: &mut Registry) {
    bind!(r, v::GET_RECORDING_DEVICE_COUNT, || capture::inputs().len() as i32);
    bind!(r, v::GET_RECORDING_DEVICE_NAME, |i: i32| StrRet::from(usize::try_from(i).ok().and_then(|i| capture::inputs().into_iter().nth(i)).map(|d| d.name)));
    bind!(r, v::GET_DEFAULT_RECORDING_DEVICE_NAME, || StrRet::from(capture::inputs().into_iter().find(|d| d.is_default).map(|d| d.name)));
    bind!(r, v::OPEN_CAPTURE, |name: Option<&CStr>, rate: i32| {
        ptr(capture::open(&name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), rate.max(1) as u32))
    });
    bind!(r, v::CLOSE_CAPTURE, destroy);
    bind!(r, v::READ_CAPTURE, |c, out: RawPtr<c_void>, max: i32| {
        let Some(c) = get::<Capture>(c) else { return 0 };
        // SAFETY: managed pins a float[max] for the call (VoiceManager.ReadVoice).
        c.read(unsafe { out.cast::<f32>().as_mut_slice(max.max(0) as usize) }) as i32
    });

    bind!(r, v::CREATE_ENCODER, |rate: i32, bitrate| ptr(Encoder::new(rate.max(0) as u32, bitrate)));
    bind!(r, v::DESTROY_ENCODER, destroy);
    bind!(r, v::ENCODE, |e, pcm: RawPtr<c_void>, samples: i32, out: RawPtr<c_void>, cap: i32| {
        let Some(e) = get::<Encoder>(e) else { return 0 };
        // SAFETY: managed pins `samples` floats in and `cap` bytes out for the call (VoiceManager.Encode).
        let (pcm, out) = unsafe { (pcm.cast::<f32>().as_slice(samples.max(0) as usize), out.cast::<u8>().as_mut_slice(cap.max(0) as usize)) };
        e.encode(pcm, out) as i32
    });
    bind!(r, v::CREATE_DECODER, |rate: i32| ptr(Decoder::new(rate.max(0) as u32)));
    bind!(r, v::DESTROY_DECODER, destroy);
    bind!(r, v::DECODE, |d, data: RawPtr<c_void>, len: i32, out: RawPtr<c_void>, cap: i32| {
        let Some(d) = get::<Decoder>(d) else { return 0 };
        // SAFETY: managed pins the `len`-byte packet and a short[cap] for the call (VoiceManager.Decode).
        let (packet, out) = unsafe { (data.cast::<u8>().as_slice(len.max(0) as usize), out.cast::<i16>().as_mut_slice(cap.max(0) as usize)) };
        d.decode(packet, out) as i32
    });
}
