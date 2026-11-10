//! The per-voice and per-mixer effects managed applies to device buffer sets: Steam Audio's
//! direct, binaural, reverb and path effects, and `CAudioProcessor`. See docs/audio.md.

use super::mix::{self, DevRef};
use abi::slots::{c_audio_processor as proc_, c_binaural_effect as binaural, c_direct_effect as direct, c_path_effect as path, c_reverb_effect as reverb};
use abi::structs::Vector3;
use abi::{NativeRef, Registry, StringToken, bind};
use audio::effects::binaural::Binaural;
use audio::effects::direct::{Direct, DirectParams};
use audio::effects::processor::Processor;
use audio::effects::reverb::Reverb;
use std::sync::Mutex;

/// Run `f` on effect `r`'s state with `input`'s blocks, writing what it leaves in `output`.
fn apply<C, T: Send + 'static>(r: NativeRef<C>, input: DevRef, output: DevRef, f: impl FnOnce(&mut T, &[audio::Block], &mut [audio::Block])) {
    let Some(effect) = objects::get::<_, Mutex<T>>(r) else { return };
    let src = mix::read_all(input);
    let mut out = audio::blocks(mix::channel_count(output));
    f(&mut effect.lock().unwrap(), &src, &mut out);
    mix::write_all(output, &out);
}

fn control_name(token: StringToken) -> Option<&'static str> {
    ["delay", "gain", "feedback", "pitchScale"].into_iter().find(|n| StringToken::of(n) == token)
}

pub fn register(r: &mut Registry) {
    bind!(r, binaural::CREATE, || objects::create(Mutex::new(Binaural::default())));
    bind!(r, binaural::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, binaural::APPLY, |r, dir: Vector3, blend, _nearest: bool, i, o| {
        apply(r, i, o, |b: &mut Binaural, src, out| b.apply([dir.vec.x, dir.vec.y, dir.vec.z], blend, src, out))
    });

    bind!(r, direct::CREATE, || objects::create(Mutex::new(Direct::default())));
    bind!(r, direct::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, direct::APPLY, |r, distance, air_l, air_m, air_h, occlusion, tx_l, tx_m, tx_h, i, o| {
        let p = DirectParams { distance, air: [air_l, air_m, air_h], occlusion, transmission: [tx_l, tx_m, tx_h] };
        apply(r, i, o, |d: &mut Direct, src, out| d.apply(&p, src, out))
    });

    bind!(r, reverb::CREATE, || objects::create(Mutex::new(Reverb::default())));
    bind!(r, reverb::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, reverb::APPLY, |r, low, mid, high, i, o| apply(r, i, o, |v: &mut Reverb, src, out| v.apply(low, mid, high, src, out)));

    // TODO: Steam Audio's path effect (managed never creates one yet); passes the input through.
    bind!(r, path::CREATE, || objects::create(Mutex::new(())));
    bind!(r, path::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, path::APPLY, |r, _, _, _, _, _, _, _, i, o| {
        apply(r, i, o, |_: &mut (), src, out| out.iter_mut().zip(src).for_each(|(o, s)| *o = *s))
    });

    bind!(r, proc_::CREATE_DELAY, |channels: i32| objects::create(Mutex::new(Processor::delay(channels.max(1) as usize))));
    bind!(r, proc_::CREATE_PITCH_SHIFT, |channels: i32| objects::create(Mutex::new(Processor::pitch_shift(channels.max(1) as usize))));
    bind!(r, proc_::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, proc_::PROCESS, |r, i, o, channels: i32| {
        apply(r, i, o, |p: &mut Processor, src, out| p.process(src, out, channels.max(0) as usize))
    });
    bind!(r, proc_::SET_CONTROL_PARAMETER, |r, name, value| {
        let Some(name) = control_name(name) else { return false };
        objects::get::<_, Mutex<Processor>>(r).is_some_and(|p| p.lock().unwrap().set_control(name, value))
    });
    bind!(r, proc_::SET_NAME_PARAMETER, |_, _, _| false);
}
