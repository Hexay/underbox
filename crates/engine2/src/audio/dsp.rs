//! `DspPreset` (processor chains managed's `DspFactory` builds once) and the `DspInstance`s a
//! `DspProcessor` runs per channel.

use super::mix;
use abi::slots::{dsp_instance as inst, dsp_preset as dsp};
use abi::{NativeRef, Registry, bind, class};
use audio::dsp::{Instance, Preset};
use std::ffi::CStr;
use std::sync::Mutex;

fn create_preset(name: Option<&CStr>) -> NativeRef<class::DspPreset> {
    let name = name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    objects::create(Mutex::new(Preset { name, processors: Vec::new() }))
}

fn preset(r: NativeRef<class::DspPreset>) -> Option<Preset> {
    objects::get::<_, Mutex<Preset>>(r).map(|p| p.lock().unwrap().clone())
}

pub fn register(r: &mut Registry) {
    bind!(r, dsp::CREATE, create_preset);
    bind!(r, dsp::DISPOSE, |p| {
        objects::destroy(p);
    });
    bind!(r, dsp::ADD_PROCESSOR, |p, kind, params: &mut [f32]| {
        if let Some(p) = objects::get::<_, Mutex<Preset>>(p) {
            p.lock().unwrap().processors.push((kind, params.to_vec()));
        }
    });
    // Instances copy the chain, so a finished preset needs nothing more.
    bind!(r, dsp::FINISH_BUILDING, |_| {});
    bind!(r, dsp::INSTANTIATE, |p, channels: i32| {
        preset(p).map_or(NativeRef::null(), |p| objects::create(Mutex::new(Instance::new(&p, channels.max(1) as usize))))
    });

    bind!(r, inst::DISPOSE, |i| {
        objects::destroy(i);
    });
    bind!(r, inst::PROCESS_CHANNEL, |i, input, output, channel: i32| {
        let Some(instance) = objects::get::<_, Mutex<Instance>>(i) else { return };
        let mut out = audio::SILENT;
        instance.lock().unwrap().process(channel.max(0) as usize, &mix::read(input), &mut out);
        mix::write(output, &out);
    });
}
