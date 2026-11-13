//! `CVideoPlayer`: what managed set on it, the `video::Player` playing its file, and the texture
//! `Update` copies due frames into. Managed callbacks are never made under the state lock: the
//! texture callback calls straight back into `GetTexture`.

use super::events::{self, Managed};
use crate::render::{device, texture};
use abi::callbacks::sandbox_video_player as cb;
use abi::structs::TextureCreationConfig_t;
use abi::{ManagedObject, NativeRef, RawPtr, class};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(super) type Ref = NativeRef<class::CVideoPlayer>;

#[derive(Default)]
pub(super) struct State {
    pub repeat: bool,
    pub muted: bool,
    pub paused: bool,
    pub video_only: bool,
    pub playing: Option<video::Player>,
    pub events: Option<Arc<Managed>>,
    /// The texture managed samples (our own strong handle) and its size.
    pub texture: Option<(texture::Ref, u32, u32)>,
}

pub(super) struct VideoPlayer {
    pub owner: ManagedObject,
    pub state: Mutex<State>,
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        let s = self.state.get_mut().unwrap();
        s.playing = None;
        if let Some(e) = s.events.take() {
            e.free_audio(false);
        }
        if let Some((t, _, _)) = s.texture.take() {
            crate::resource::destroy_handle(t);
        }
    }
}

pub(super) fn create(owner: ManagedObject) -> Ref {
    objects::create(VideoPlayer { owner, state: Mutex::default() })
}

pub(super) fn with<R: Default>(p: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    objects::get::<_, VideoPlayer>(p).map(|v| f(&mut v.state.lock().unwrap())).unwrap_or_default()
}

/// With the playing file, if any.
pub(super) fn playing<R: Default>(p: Ref, f: impl FnOnce(&video::Player) -> R) -> R {
    with(p, |s| s.playing.as_ref().map(f).unwrap_or_default())
}

/// `Play`: start `url` (a full path from managed's filesystem, or a path the native filesystem
/// resolves). False for http(s) URLs, which aren't fetched (docs/video.md), and missing files.
pub(super) fn play(p: Ref, url: &str) -> bool {
    let Some(v) = objects::get::<_, VideoPlayer>(p) else { return false };
    if url.starts_with("http://") || url.starts_with("https://") {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            abi::rt::log::warn(&format!("video: streaming from URLs isn't supported; '{url}' won't play"));
        }
        return false;
    }
    let path = Path::new(url);
    let read: Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send> = if path.is_absolute() {
        let path = crate::fs::on_disk(path);
        if !path.is_file() {
            return false;
        }
        Box::new(move || std::fs::read(&path).map_err(|e| format!("video: {}: {e}", path.display())))
    } else {
        let url = url.to_string();
        Box::new(move || crate::fs::read_unreported(&url, "GAME").ok_or_else(|| format!("video: '{url}' not found")))
    };
    stop(p, true);
    let events = Arc::new(Managed::new(v.owner));
    let mut s = v.state.lock().unwrap();
    s.paused = false;
    let options = video::Options { decode_video: device::gpu().is_some(), decode_audio: !s.video_only, repeat: s.repeat, muted: s.muted, paused: false };
    s.playing = Some(video::Player::load(read, options, events.clone()));
    s.events = Some(events);
    true
}

/// `Stop`: end playback and free the audio track; the last frame stays in the texture.
pub(super) fn stop(p: Ref, notify: bool) {
    let (playing, events) = with(p, |s| {
        s.paused = false;
        (s.playing.take(), s.events.take())
    });
    drop(playing);
    if let Some(e) = events {
        e.free_audio(notify);
    }
}

/// `Update` (managed's `Present`): show the newest due frame, through `OnTextureData` when managed
/// wants the pixels, else in the texture, made (and announced) at the first frame's size.
pub(super) fn update(p: Ref) {
    let Some(v) = objects::get::<_, VideoPlayer>(p) else { return };
    let (frame, current) = {
        let s = v.state.lock().unwrap();
        let Some(playing) = &s.playing else { return };
        (playing.take_frame(), s.texture)
    };
    let Some(mut frame) = frame else { return };
    if events::managed(events::WANTS_TEXTURE_DATA) && cb::wants_texture_data(v.owner) {
        let data = RawPtr::from_raw(frame.rgba.as_mut_ptr().cast());
        cb::on_texture_data_internal(v.owner, data, frame.width as i32, frame.height as i32);
        return;
    }
    match current {
        Some((t, w, h)) if (w, h) == (frame.width, frame.height) => texture::replace(t, &frame.rgba),
        _ => {
            let t = new_texture(frame.width, frame.height, &mut frame.rgba);
            let old = with(p, |s| s.texture.replace((t, frame.width, frame.height)));
            if let Some((old, _, _)) = old {
                crate::resource::destroy_handle(old);
            }
            if events::managed(events::TEXTURE_CREATED) {
                cb::on_texture_created_internal(v.owner);
            }
        }
    }
}

fn new_texture(width: u32, height: u32, rgba: &mut [u8]) -> texture::Ref {
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (width as _, height as _, 1, 1);
    // ImageFormat RGBA8888.
    config.m_n_image_format = 0;
    texture::create(Some(c"video"), true, config, rgba)
}

/// `GetWidth`/`GetHeight`: the texture's size once a frame is up, else the container's.
pub(super) fn size(p: Ref) -> (i32, i32) {
    with(p, |s| match (s.texture, s.playing.as_ref().and_then(video::Player::info)) {
        (Some((_, w, h)), _) => (w as i32, h as i32),
        (None, Some(i)) => (i.width as i32, i.height as i32),
        _ => (0, 0),
    })
}
