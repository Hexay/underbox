//! `CVideoPlayer` and `CVideoRecorder` without a codec: nothing decodes or encodes, so a player
//! never loads (managed keeps its 1x1 placeholder texture, `IsLoaded` false, no `OnLoaded`), and a
//! recorder never becomes ready for frames. The player still keeps the state managed sets on it.
// TODO: a decoder (mp4/H.264 via openh264, webm/AV1 via rav1d) once render exposes texture uploads.

use abi::slots::{c_video_player as vp, c_video_recorder as vr};
use abi::{HandleId, ManagedObject, NativeRef, Registry, StrRet, bind, class};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Player = NativeRef<class::CVideoPlayer>;

/// What managed set on a player; there is never a stream behind it.
#[derive(Default)]
struct State {
    repeat: bool,
    muted: bool,
    paused: bool,
}

struct VideoPlayer {
    _owner: ManagedObject,
    state: Mutex<State>,
}

fn with<R: Default>(p: Player, f: impl FnOnce(&mut State) -> R) -> R {
    objects::get::<_, VideoPlayer>(p).map(|v| f(&mut v.state.lock().unwrap())).unwrap_or_default()
}

fn play(p: Player, url: Option<&std::ffi::CStr>) -> bool {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        let url = url.map(|u| u.to_string_lossy()).unwrap_or_default();
        abi::rt::log::warn(&format!("video playback isn't available in this build; '{url}' won't play"));
    }
    with(p, |s| s.paused = false);
    false
}

fn player(r: &mut Registry) {
    bind!(r, vp::CREATE, |owner| objects::create(VideoPlayer { _owner: owner, state: Mutex::default() }));
    bind!(r, vp::DESTROY, |p| {
        objects::destroy(p);
    });
    bind!(r, vp::PLAY, |p, url, _| play(p, url));
    bind!(r, vp::RESUME, |p| with(p, |s| s.paused = false));
    bind!(r, vp::PAUSE, |p| with(p, |s| s.paused = true));
    bind!(r, vp::STOP, |p| with(p, |s| s.paused = false));
    bind!(r, vp::SEEK, |_, _| {});
    bind!(r, vp::GET_REPEAT, |p| with(p, |s| s.repeat));
    bind!(r, vp::SET_REPEAT, |p, v| with(p, |s| s.repeat = v));
    bind!(r, vp::IS_PAUSED, |p| with(p, |s| s.paused));
    bind!(r, vp::IS_MUTED, |p| with(p, |s| s.muted));
    bind!(r, vp::SET_MUTED, |p, v| with(p, |s| s.muted = v));
    bind!(r, vp::SET_VIDEO_ONLY, |_| {});
    bind!(r, vp::UPDATE, |_| {});

    // No stream: no length, size, audio, metadata or levels.
    bind!(r, vp::GET_DURATION, |_| 0.0);
    bind!(r, vp::GET_PLAYBACK_TIME, |_| 0.0);
    bind!(r, vp::HAS_AUDIO_STREAM, |_| false);
    bind!(r, vp::GET_WIDTH, |_| 0);
    bind!(r, vp::GET_HEIGHT, |_| 0);
    bind!(r, vp::GET_METADATA, |_, _| StrRet::from(c""));
    // Managed sizes its array from the vector's count, so leaving it empty is "no spectrum".
    bind!(r, vp::GET_SPECTRUM, |_, _| {});
    bind!(r, vp::GET_AMPLITUDE, |_| 0.0);
    bind!(r, vp::GET_VIDEO_DECODE_SECONDS, |_| 0.0);
    bind!(r, vp::GET_AUDIO_DECODE_SECONDS, |_| 0.0);
    bind!(r, vp::GET_DECODED_FRAME_COUNT, |_| 0);
    bind!(r, vp::GET_PACKET_QUEUE_DEPTH, |_| 0);
    bind!(r, vp::GET_FRAME_QUEUE_DEPTH, |_| 0);
    // Only asked for after OnTextureCreated/OnInitAudio, which never fire.
    bind!(r, vp::GET_TEXTURE, |_| NativeRef::null());
    bind!(r, vp::GET_AUDIO_STREAM, |_| HandleId::new(0));
}

struct VideoRecorder;

fn recorder(r: &mut Registry) {
    bind!(r, vr::CREATE, || objects::create(VideoRecorder));
    bind!(r, vr::DESTROY, |v| {
        objects::destroy(v);
    });
    bind!(r, vr::INITIALIZE, |_, path, _, _, _, _, _, _, _, _, _, _, _| {
        let path = path.map(|p| p.to_string_lossy()).unwrap_or_default();
        abi::rt::log::warn(&format!("video recording isn't available in this build; '{path}' won't be written"));
        false
    });
    bind!(r, vr::READY_FOR_VIDEO_FRAME, |_| false);
    bind!(r, vr::ADD_VIDEO_FRAME, |_, _, _| {});
    bind!(r, vr::ADD_AUDIO_SAMPLES, |_, _| {});
    bind!(r, vr::STOP, |_| {});
}

pub fn register(r: &mut Registry) {
    player(r);
    recorder(r);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_keeps_settings_but_never_plays() {
        let p: Player = objects::create(VideoPlayer { _owner: ManagedObject(1), state: Mutex::default() });
        with(p, |s| s.repeat = true);
        with(p, |s| s.paused = true);
        assert!(!play(p, Some(c"https://example.com/a.mp4")));
        assert!(with(p, |s| s.repeat) && !with(p, |s| s.paused));
        assert!(objects::destroy(p));
        assert!(!with(p, |s| s.repeat), "a destroyed player reads as default");
    }
}
