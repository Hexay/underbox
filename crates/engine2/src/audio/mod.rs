//! Audio. No output device yet (TODO: M2 mixer + device); DSP presets are kept as the
//! processor chains managed builds, ready for the mixer to instantiate.

use abi::slots::{dsp_preset as dsp, g_p_audio_device as device};
use abi::{NativeRef, Registry, StrRet, bind, class};
use std::ffi::CStr;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Processor {
    pub kind: i32,
    pub params: Vec<f32>,
}

#[derive(Default)]
pub struct DspPreset {
    pub name: String,
    pub processors: Mutex<Vec<Processor>>,
    pub finished: Mutex<bool>,
}

fn create_preset(name: Option<&CStr>) -> NativeRef<class::DspPreset> {
    let name = name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    objects::create(DspPreset { name, ..Default::default() })
}

pub mod mix;

pub fn register(r: &mut Registry) {
    mix::register(r);
    bind!(r, device::IS_VALID, || false);
    bind!(r, device::IS_ACTIVE, || false);
    bind!(r, device::NAME, || StrRet::from(c"none"));

    bind!(r, dsp::CREATE, create_preset);
    bind!(r, dsp::DISPOSE, |p| {
        objects::destroy(p);
    });
    bind!(r, dsp::ADD_PROCESSOR, |p, kind, params: &mut [f32]| {
        if let Some(p) = objects::get::<_, DspPreset>(p) {
            p.processors.lock().unwrap().push(Processor { kind, params: params.to_vec() });
        }
    });
    bind!(r, dsp::FINISH_BUILDING, |p| {
        if let Some(p) = objects::get::<_, DspPreset>(p) {
            *p.finished.lock().unwrap() = true;
        }
    });
}
