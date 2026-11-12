//! `g_pAudioDevice` and the device half of `g_pSoundSystem`. The device opens on managed's first
//! query and mixing starts on its first `MuteDevice`; see docs/audio.md, "Devices" and "Per frame".

use super::mix::{self, DevRef};
use crate::app::dict;
use abi::slots::{g_p_audio_device as dev, g_p_sound_system_internal as system};
use abi::structs::AudioDeviceDesc;
use abi::{Registry, StrRet, bind};
use audio::output::{Queue, Worker};
use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No device: servers, test apps, `UNDERBOX_AUDIO=off`.
    Off,
    /// A clock drains the queue in real time (`UNDERBOX_AUDIO=null`, `SDL_AUDIO_DRIVER=dummy`).
    Null,
    /// The null clock, writing what it plays to `SDL_AUDIO_DISK_OUTPUT_FILE` (`SDL_AUDIO_DRIVER=disk`).
    Disk,
    /// cpal's output.
    Real,
}

pub fn mode() -> Mode {
    let config = dict::current_config();
    // Real engine2 opens audio through SDL3, so SDL's own driver choice applies to it as well.
    let sdl = std::env::var("SDL_AUDIO_DRIVER").or_else(|_| std::env::var("SDL_AUDIODRIVER"));
    match (std::env::var("UNDERBOX_AUDIO").as_deref(), sdl.as_deref()) {
        _ if config.dedicated_server || config.test_mode => Mode::Off,
        (Ok("off" | "none"), _) => Mode::Off,
        (Ok("null"), _) => Mode::Null,
        (Ok("disk"), _) | (Err(_), Ok("disk")) => Mode::Disk,
        (Err(_), Ok("dummy")) => Mode::Null,
        _ if cfg!(feature = "audio-device") => Mode::Real,
        _ => Mode::Off,
    }
}

/// SDL's default when `SDL_AUDIO_DISK_OUTPUT_FILE` is unset.
fn disk_path() -> String {
    std::env::var("SDL_AUDIO_DISK_OUTPUT_FILE").unwrap_or_else(|_| "sdlaudio.raw".into())
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
        Mode::Disk => {
            let path = disk_path();
            let file = match std::fs::File::create(&path) {
                Ok(f) => std::io::BufWriter::new(f),
                Err(e) => {
                    abi::rt::log::warn(&format!("audio disk device: {path}: {e}"));
                    return None;
                }
            };
            let queue = Arc::new(Queue::new(2));
            (queue.clone(), format!("disk {path}"), Backend::Null(audio::output::spawn_disk_clock(queue, file)))
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

/// Hand managed's `MixOneBuffer` to the queue (the device mixes through it when it runs short)
/// and start the feeder that keeps it mixed ahead (once).
fn start_mixing(d: &Device) {
    let mut feeder = d.feeder.lock().unwrap();
    if feeder.is_none() {
        d.queue.set_mixer(Some(Arc::new(abi::callbacks::sandbox_audio_mixing_thread::mix_one_buffer)));
        *feeder = Some(audio::output::spawn_feeder(d.queue.clone()));
    }
}

/// Stop every call into managed's mixer, waiting out one in progress. True if it was mixing.
fn stop_mixing(d: &Device) -> bool {
    let feeder = d.feeder.lock().unwrap().take();
    let mixing = feeder.is_some();
    drop(feeder);
    d.queue.set_mixer(None);
    mixing
}

/// Underrun frames already logged.
static UNDERRUNS_LOGGED: AtomicU64 = AtomicU64::new(0);

fn send_output(buffers: DevRef) {
    let blocks = mix::read_all(buffers);
    super::stats::output_sent(&blocks);
    with(|d| {
        d.queue.push(&blocks);
        let (runs, frames) = d.queue.underruns();
        let before = UNDERRUNS_LOGGED.swap(frames, Relaxed);
        if frames > before {
            let ms = |f: u64| f as f64 * 1000.0 / audio::RATE as f64;
            abi::rt::log::warn(&format!("audio device {} underran: {:.1} ms of silence ({runs} underruns, {:.1} ms in all)", d.name, ms(frames - before), ms(frames)));
        }
    });
}

fn set_active(id: Option<&CStr>) {
    let id = id.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    *ACTIVE.lock().unwrap() = id.clone();
    let Some(old) = DEVICE.read().unwrap().clone() else { return };
    // Outside DEVICE's lock: the feeder and the device thread mix, and a mix's SendOutput reads DEVICE.
    let mixing = stop_mixing(&old);
    let mut slot = DEVICE.write().unwrap();
    slot.take();
    drop(old);
    UNDERRUNS_LOGGED.store(0, Relaxed);
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
        let info = device().map_or_else(|| "no audio device".into(), |d| format!("audio device {}: {} channels, {} frames queued, {} ahead, underruns {:?} (count, frames)", d.name, d.queue.channels, d.queue.queued_frames(), d.queue.ahead_frames(), d.queue.underruns()));
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
