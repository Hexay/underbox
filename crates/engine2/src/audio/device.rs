//! `g_pAudioDevice` and the device half of `g_pSoundSystem`. The device opens on managed's first
//! query and mixing starts on its first `MuteDevice`; see docs/audio.md, "Devices" and "Per frame".

use super::mix::{self, DevRef};
use crate::app::dict;
use abi::slots::{g_p_audio_device as dev, g_p_sound_system_internal as system};
use abi::structs::AudioDeviceDesc;
use abi::{Registry, StrRet, bind};
use audio::output::{Queue, Worker};
use std::ffi::CStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No device: servers, test apps, `UNDERBOX_AUDIO=off`.
    Off,
    /// A clock drains the queue in real time (`UNDERBOX_AUDIO=null`).
    Null,
    /// cpal's output.
    Real,
}

pub fn mode() -> Mode {
    let config = dict::current_config();
    match std::env::var("UNDERBOX_AUDIO").as_deref() {
        _ if config.dedicated_server || config.test_mode => Mode::Off,
        Ok("off" | "none") => Mode::Off,
        Ok("null") => Mode::Null,
        _ if cfg!(feature = "audio-device") => Mode::Real,
        _ => Mode::Off,
    }
}

enum Backend {
    Null(#[allow(dead_code)] Worker),
    #[cfg(feature = "audio-device")]
    Real(#[allow(dead_code)] audio::device::Output),
}

pub struct Device {
    pub queue: Arc<Queue>,
    name: String,
    feeder: Mutex<Option<Worker>>,
    _backend: Backend,
}

/// The open device; `None` until the first query and whenever there's none.
static DEVICE: RwLock<Option<Arc<Device>>> = RwLock::new(None);
/// The device managed picked (`SetActiveAudioDevice`); empty is the system default.
static ACTIVE: Mutex<String> = Mutex::new(String::new());

fn open(mode: Mode, id: &str) -> Option<Device> {
    let (queue, name, backend) = match mode {
        Mode::Off => return None,
        Mode::Null => {
            let queue = Arc::new(Queue::new(2));
            (queue.clone(), "null".to_owned(), Backend::Null(audio::output::spawn_null_clock(queue)))
        }
        #[cfg(feature = "audio-device")]
        Mode::Real => match audio::device::open_output(Some(id)) {
            Ok(out) => (out.queue.clone(), out.name.clone(), Backend::Real(out)),
            Err(e) => {
                abi::rt::log::warn(&format!("audio device unavailable: {e}"));
                return None;
            }
        },
        #[cfg(not(feature = "audio-device"))]
        Mode::Real => return None,
    };
    let _ = id;
    abi::rt::log::log(abi::rt::log::Level::Info, &format!("audio device: {name}, {} channels", queue.channels));
    Some(Device { queue, name, feeder: Mutex::new(None), _backend: backend })
}

/// The device, opening it on first use.
pub fn device() -> Option<Arc<Device>> {
    if let Some(d) = DEVICE.read().unwrap().clone() {
        return Some(d);
    }
    let mode = mode();
    if mode == Mode::Off {
        return None;
    }
    let mut slot = DEVICE.write().unwrap();
    if slot.is_none() {
        *slot = open(mode, &ACTIVE.lock().unwrap()).map(Arc::new);
    }
    slot.clone()
}

fn with<R: Default>(f: impl FnOnce(&Device) -> R) -> R {
    device().map(|d| f(&d)).unwrap_or_default()
}

/// Start the feeder that calls managed's `MixOneBuffer` (once).
fn start_mixing(d: &Device) {
    let mut feeder = d.feeder.lock().unwrap();
    if feeder.is_none() {
        *feeder = Some(audio::output::spawn_feeder(d.queue.clone(), abi::callbacks::sandbox_audio_mixing_thread::mix_one_buffer));
    }
}

fn send_output(buffers: DevRef) {
    with(|d| d.queue.push(&mix::read_all(buffers)));
}

fn set_active(id: Option<&CStr>) {
    let id = id.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    *ACTIVE.lock().unwrap() = id.clone();
    let mut slot = DEVICE.write().unwrap();
    let Some(old) = slot.take() else { return };
    let mixing = old.feeder.lock().unwrap().take().is_some();
    drop(old);
    *slot = open(mode(), &id).map(Arc::new);
    if let (Some(d), true) = (slot.as_ref(), mixing) {
        start_mixing(d);
    }
}

#[cfg(feature = "audio-device")]
fn outputs() -> Vec<audio::DeviceInfo> {
    if mode() == Mode::Real { audio::device::outputs() } else { Vec::new() }
}

#[cfg(not(feature = "audio-device"))]
fn outputs() -> Vec<audio::DeviceInfo> {
    Vec::new()
}

pub fn register(r: &mut Registry) {
    bind!(r, dev::IS_VALID, || device().is_some());
    bind!(r, dev::NAME, || StrRet::from(device().map(|d| d.name.clone()).unwrap_or_else(|| "none".into())));
    bind!(r, dev::CHANNEL_COUNT, || with(|d| d.queue.channels as u32));
    bind!(r, dev::MIX_CHANNEL_COUNT, || with(|d| d.queue.channels as u32));
    bind!(r, dev::BITS_PER_SAMPLE, || with(|_| 32));
    bind!(r, dev::BYTES_PER_SAMPLE, || with(|_| 4));
    bind!(r, dev::SAMPLE_RATE, || with(|_| audio::RATE));
    bind!(r, dev::IS_ACTIVE, || with(|d| d.feeder.lock().unwrap().is_some() && !d.queue.is_muted()));
    bind!(r, dev::CANCEL_OUTPUT, || with(|d| d.queue.clear()));
    bind!(r, dev::CLEAR_BUFFER, || with(|d| d.queue.clear()));
    bind!(r, dev::WAIT_FOR_COMPLETE, || with(|d| d.queue.wait_empty(Duration::from_secs(1))));
    bind!(r, dev::MUTE_DEVICE, |mute| {
        with(|d| {
            d.queue.set_muted(mute);
            start_mixing(d);
        })
    });
    bind!(r, dev::OUTPUT_DEBUG_INFO, || {
        let info = device().map_or_else(|| "no audio device".into(), |d| format!("audio device {}: {} channels, {} frames queued, {} ahead", d.name, d.queue.channels, d.queue.queued_frames(), d.queue.ahead_frames()));
        abi::rt::log::log(abi::rt::log::Level::Info, &info);
    });
    bind!(r, dev::SET_MIX_AHEAD, |seconds| with(|d| d.queue.set_ahead_seconds(seconds)));
    bind!(r, dev::SEND_OUTPUT, send_output);

    bind!(r, system::GET_NUM_AUDIO_DEVICES, || outputs().len() as i32);
    bind!(r, system::GET_AUDIO_DEVICE_DESC, |i: i32| {
        let d = usize::try_from(i).ok().and_then(|i| outputs().into_iter().nth(i));
        d.map_or_else(AudioDeviceDesc::default, |d| AudioDeviceDesc { is_available: 1, is_default: d.is_default as u8 })
    });
    bind!(r, system::GET_AUDIO_DEVICE_ID, |i: i32| StrRet::from(usize::try_from(i).ok().and_then(|i| outputs().into_iter().nth(i)).map(|d| d.id)));
    bind!(r, system::GET_AUDIO_DEVICE_NAME, |i: i32| StrRet::from(usize::try_from(i).ok().and_then(|i| outputs().into_iter().nth(i)).map(|d| d.name)));
    bind!(r, system::GET_ACTIVE_AUDIO_DEVICE, || StrRet::from(ACTIVE.lock().unwrap().clone()));
    bind!(r, system::SET_ACTIVE_AUDIO_DEVICE, set_active);
    // Editor-only (Sandbox.Tools): out of scope, see docs/PLAN.md.
    bind!(r, system::PLAY_SOUND_AT_OS_LEVEL, |path: Option<&CStr>| abi::rt::log::warn(&format!("PlaySoundAtOSLevel({path:?}): editor-only, not supported")));
}
