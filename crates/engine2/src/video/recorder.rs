//! `CVideoRecorder` over `video::recorder`: H.264 in MP4 whatever codec managed names, since
//! that's the encoder there is (docs/video.md). Other containers fail `Initialize`.

use abi::slots::c_video_recorder as vr;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::ffi::{CStr, c_void};
use std::path::Path;
use std::sync::Mutex;
use video::recorder::{Config, Recorder};

type Ref = NativeRef<class::CVideoRecorder>;

#[derive(Default)]
struct VideoRecorder {
    writer: Mutex<Option<Recorder>>,
    frame_len: Mutex<usize>,
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&VideoRecorder) -> R) -> R {
    objects::get::<_, VideoRecorder>(r).map(|v| f(&v)).unwrap_or_default()
}

pub(super) fn initialize(r: Ref, path: &str, config: Config, codec: &str, container: &str) -> bool {
    if !container.eq_ignore_ascii_case("mp4") {
        abi::rt::log::warn(&format!("video recording writes MP4 only; '{container}' for '{path}' isn't available"));
        return false;
    }
    if !codec.eq_ignore_ascii_case("h264") {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("video recording: writing H.264 in place of '{codec}'"));
    }
    match Recorder::create(Path::new(path), config) {
        Ok(rec) => with(r, |v| {
            *v.frame_len.lock().unwrap() = (config.width * config.height * 4) as usize;
            *v.writer.lock().unwrap() = Some(rec);
            true
        }),
        Err(e) => {
            abi::rt::log::warn(&e);
            false
        }
    }
}

pub(super) fn create() -> Ref {
    objects::create(VideoRecorder::default())
}

fn add_frame(r: Ref, rgba: RawPtr<c_void>, micros: i64) {
    let len = with(r, |v| *v.frame_len.lock().unwrap());
    if rgba.is_null() || len == 0 {
        return;
    }
    // SAFETY: managed pins width * height * 4 bytes for the call (VideoWriter.AddFrame checks the length).
    add(r, unsafe { rgba.cast::<u8>().as_slice(len) }, micros);
}

/// Queue one RGBA frame; blocks while the encoder is behind.
pub(super) fn add(r: Ref, rgba: &[u8], micros: i64) {
    with(r, |v| {
        if let Some(rec) = &*v.writer.lock().unwrap() {
            rec.add_frame(rgba, micros);
        }
    });
}

/// Flush the encoder and write the file's index.
pub(super) fn finish(r: Ref) {
    let rec = with(r, |v| v.writer.lock().unwrap().take());
    if let Some(Err(e)) = rec.map(Recorder::finish) {
        abi::rt::log::warn(&e);
    }
}

pub(super) fn register(r: &mut Registry) {
    bind!(r, vr::CREATE, create);
    bind!(r, vr::DESTROY, |v| {
        finish(v);
        objects::destroy(v);
    });
    bind!(r, vr::INITIALIZE, |v, path, w, h, fps, mbps, _rate, _channels, codec, container, _preset, _audio, _alpha| {
        let config = Config { width: w.max(0) as u32, height: h.max(0) as u32, fps: fps.max(1) as u32, bitrate_mbps: mbps.max(1) as u32 };
        initialize(v, &text(path), config, &text(codec), &text(container))
    });
    bind!(r, vr::READY_FOR_VIDEO_FRAME, |v| with(v, |v| v.writer.lock().unwrap().as_ref().is_some_and(Recorder::ready)));
    bind!(r, vr::ADD_VIDEO_FRAME, add_frame);
    // TODO: audio in recordings (managed hands mixer buffers; there's no AAC/Opus-in-MP4 writer yet).
    bind!(r, vr::ADD_AUDIO_SAMPLES, |_, _| {});
    bind!(r, vr::STOP, finish);
}
