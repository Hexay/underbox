//! Sounds: `CSfxTable` (a loaded or loadable sound; managed's `SoundFile` and streams hold one),
//! its `VSound_t` strong handles, and `g_pSoundSystem`'s precache/create. Files decode on first
//! use, so a server that never plays a sound never decodes one.

use abi::slots::{c_sfx_table as sfx, g_p_sound_system_internal as system, v_sound_t as vsound};
use abi::{Guid, NativeRef, RawPtr, Registry, bind, class};
use audio::pcm::{Format, Pcm, RawFormat};
use audio::sampler::{Sampler, Source};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

pub type SfxRef = NativeRef<class::CSfxTable>;
type SoundRef = NativeRef<class::VSound_t>;

/// Where a precached sound decodes from.
enum Origin {
    File(PathBuf),
    /// A `.vsnd_c` resource path.
    Compiled(String),
}

enum Load {
    Pending(Option<Origin>),
    Ready(Source),
    Failed,
}

pub struct Sfx {
    pub name: String,
    load: Mutex<Load>,
}

impl Sfx {
    pub fn ready(name: &str, source: Source) -> Arc<Self> {
        Arc::new(Self { name: name.into(), load: Mutex::new(Load::Ready(source)) })
    }

    /// The decoded source, decoding it now if it hasn't been.
    pub fn source(&self) -> Option<Source> {
        let mut load = self.load.lock().unwrap();
        if let Load::Pending(origin) = &*load {
            let pcm = match origin {
                Some(Origin::File(p)) => decode_file(p),
                Some(Origin::Compiled(p)) => super::compiled::decode(p),
                None => None,
            };
            *load = pcm.map_or(Load::Failed, |pcm| Load::Ready(Source::Pcm(Arc::new(pcm))));
        }
        match &*load {
            Load::Ready(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// `SoundFile.CacheStatus` without loading: 0 not loaded, 1 loaded, 2 failed.
    fn cache_status(&self) -> i32 {
        match &*self.load.lock().unwrap() {
            Load::Pending(_) => 0,
            Load::Ready(_) => 1,
            Load::Failed => 2,
        }
    }
}

fn decode_file(path: &std::path::Path) -> Option<Pcm> {
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    audio::decode::decode(&bytes, ext.as_deref())
        .inspect_err(|e| abi::rt::log::warn(&format!("sound {}: {e}", path.display())))
        .ok()
}

/// A `.vsnd` resource path's source file (the same path with an audio extension), else its
/// compiled `.vsnd_c`.
fn resolve(name: &str) -> Option<Origin> {
    let stem = name.strip_suffix(".vsnd").or_else(|| name.strip_suffix(".vsnd_c")).unwrap_or(name);
    let file = audio::decode::SOURCE_EXTENSIONS.iter().find_map(|ext| crate::fs::resolve(&format!("{stem}.{ext}"), "GAME"));
    file.map(Origin::File).or_else(|| super::compiled::path(stem).map(Origin::Compiled))
}

/// Tables live as long as the process (managed has no slot to free one), so one per name.
static PRECACHED: LazyLock<Mutex<HashMap<String, SfxRef>>> = LazyLock::new(Mutex::default);

fn precache(name: Option<&CStr>) -> SfxRef {
    let name = name.map(|s| s.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let key = name.to_ascii_lowercase();
    let mut cache = PRECACHED.lock().unwrap();
    if let Some(r) = cache.get(&key).filter(|r| objects::get::<_, Sfx>(**r).is_some()) {
        return *r;
    }
    // Real returns null for a missing file, and for every sound on a dedicated server (tap, 2026-11-12).
    if crate::app::dict::current_config().dedicated_server {
        return SfxRef::null();
    }
    let Some(path) = resolve(&name) else {
        abi::rt::log::warn(&format!("sound {name}: no source or compiled file"));
        return SfxRef::null();
    };
    let r = objects::create_shared(Arc::new(Sfx { name, load: Mutex::new(Load::Pending(Some(path))) }));
    cache.insert(key, r);
    r
}

#[allow(clippy::too_many_arguments)]
fn create_sound(name: Option<&CStr>, channels: i32, rate: i32, format: i32, frames: i32, _duration: f32, loop_start: i32, loop_end: i32, data: &[u8]) -> SfxRef {
    let name = name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(format) = RawFormat::from_code(format) else { return SfxRef::null() };
    let Some(mut pcm) = Pcm::from_raw(format, channels.max(0) as usize, rate.max(0) as u32, frames.max(0) as usize, data) else {
        return SfxRef::null();
    };
    pcm.loop_start = usize::try_from(loop_start).ok();
    pcm.loop_end = usize::try_from(loop_end).ok().filter(|&e| e > 0);
    objects::create_shared(Sfx::ready(&name, Source::Pcm(Arc::new(pcm))))
}

pub fn sfx(r: SfxRef) -> Option<Arc<Sfx>> {
    objects::get::<_, Sfx>(r)
}

fn source(r: SfxRef) -> Option<Source> {
    sfx(r).and_then(|s| s.source())
}

/// A `VSound_t` strong handle: the sound as it was when the handle was taken.
struct Sound {
    name: String,
    source: Option<Source>,
}

fn sound<R: Default>(r: SoundRef, f: impl FnOnce(&Source) -> R) -> R {
    objects::get::<_, Sound>(r).and_then(|s| s.source.as_ref().map(f)).unwrap_or_default()
}

fn pcm_info(s: &Source) -> (Format, i32, f32) {
    match s {
        Source::Pcm(p) => (p.format, p.bits as i32, p.duration()),
        Source::Stream(_) => (Format::Pcm16, 16, 0.0),
    }
}

/// Bytes per stored sample: what the sound was decoded from, as Source 2 reports it.
fn bytes_per_sample(s: &Source) -> i32 {
    (pcm_info(s).1 / 8).max(1)
}

fn guid(name: &str) -> Guid {
    let h = abi::StringToken::of(name).0.to_le_bytes();
    Guid(std::array::from_fn(|i| h[i % 4] ^ i as u8))
}

fn get_samples(r: SfxRef, out: RawPtr<c_void>, count: u32, interleaved: bool) -> bool {
    let Some(Source::Pcm(pcm)) = source(r) else { return false };
    let samples = pcm.to_i16(interleaved);
    // SAFETY: managed pins a short[count] for the call (SoundFile.GetSamples).
    let out = unsafe { out.cast::<i16>().as_mut_slice(count as usize) };
    let n = out.len().min(samples.len());
    out[..n].copy_from_slice(&samples[..n]);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, system::PRECACHE_SOUND, precache);
    bind!(r, system::CREATE_SOUND, |name, ch, rate, fmt, frames, dur, ls, le, data: RawPtr<c_void>, len: i32| {
        // SAFETY: managed pins `len` bytes of PCM for the call (SoundFile.Create).
        let data = unsafe { data.cast::<u8>().as_slice(len.max(0) as usize) };
        create_sound(name, ch, rate, fmt, frames, dur, ls, le, data)
    });
    bind!(r, system::PRELOAD_SOUND, |s| {
        source(s);
    });

    bind!(r, sfx::GET_CACHE_STATUS, |s| sfx(s).map_or(2, |s| s.cache_status()));
    bind!(r, sfx::GET_SAMPLE_COUNT, |s| match source(s) {
        Some(Source::Pcm(p)) => p.frames() as i32,
        _ => 0,
    });
    bind!(r, sfx::GET_SAMPLES, |s, out, n| get_samples(s, out, n, false));
    bind!(r, sfx::GET_SAMPLES_INTERLEAVED, |s, out, n| get_samples(s, out, n, true));
    bind!(r, sfx::GET_SOUND, |s| {
        let Some(t) = sfx(s) else { return SoundRef::null() };
        objects::create(Sound { name: t.name.clone(), source: t.source() })
    });
    bind!(r, sfx::IS_VALID_FOR_PLAYBACK, |s| source(s).is_some());
    bind!(r, sfx::FAILED_RESOURCE_LOAD, |s| sfx(s).is_none_or(|s| s.source().is_none()));
    bind!(r, sfx::CREATE_MIXER, |s| {
        let src = source(s);
        super::stats::voice_started(&sfx(s).map(|t| t.name.clone()).unwrap_or_default(), src.is_some());
        src.map_or(NativeRef::null(), |src| crate::audio::mixer::create(Sampler::new(src), s))
    });

    bind!(r, vsound::DESTROY_STRONG_HANDLE, |s| {
        objects::destroy(s);
    });
    bind!(r, vsound::IS_STRONG_HANDLE_VALID, |s| objects::get::<_, Sound>(s).is_some());
    bind!(r, vsound::IS_ERROR, |s| objects::get::<_, Sound>(s).is_none_or(|s| s.source.is_none()));
    bind!(r, vsound::IS_STRONG_HANDLE_LOADED, |s| objects::get::<_, Sound>(s).is_some_and(|s| s.source.is_some()));
    bind!(r, vsound::COPY_STRONG_HANDLE, |s| {
        objects::get::<_, Sound>(s).map_or(SoundRef::null(), |s| objects::create(Sound { name: s.name.clone(), source: s.source.clone() }))
    });
    bind!(r, vsound::GET_BINDING_PTR, |s: SoundRef| RawPtr::from_raw(s.as_raw()));
    bind!(r, vsound::GET_GUID, |s| objects::get::<_, Sound>(s).map_or(Guid([0; 16]), |s| guid(&s.name)));
    bind!(r, vsound::FORMAT, |s| sound(s, |s| pcm_info(s).0 as i64));
    bind!(r, vsound::BITS_PER_SAMPLE, |s| sound(s, |s| pcm_info(s).1));
    bind!(r, vsound::CHANNELS, |s| sound(s, |s| s.channels() as i32));
    bind!(r, vsound::BYTES_PER_SAMPLE, |s| sound(s, bytes_per_sample));
    bind!(r, vsound::M_SAMPLE_FRAME_SIZE, |s| sound(s, |s| bytes_per_sample(s) * s.channels() as i32));
    bind!(r, vsound::M_RATE, |s| sound(s, |s| s.rate() as i32));
    bind!(r, vsound::DURATION, |s| sound(s, |s| pcm_info(s).2));
}
