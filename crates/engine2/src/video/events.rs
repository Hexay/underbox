//! A player's worker talking to managed: `Sandbox.VideoPlayer`'s callbacks and its audio stream.
//! The worker only calls callbacks managed queues to its main thread (OnLoaded, OnInitAudio,
//! OnFinished, OnRepeated); the `CAudioStreamManaged` handle is made on the main thread, when
//! managed asks for it from `OnAudioReadyInternal`.

use crate::audio::stream as managed_stream;
use abi::callbacks::sandbox_video_player as cb;
use abi::{HandleId, ManagedObject, class};
use audio::stream::Stream;
use std::sync::{Arc, Mutex};
use video::{AudioSink, Events, Info};

/// Callback table indices (abi's callbacks.rs), checked before calling: unit tests have no managed.
const INIT_AUDIO: usize = 63;
const FREE_AUDIO: usize = 64;
const LOADED: usize = 66;
pub(super) const WANTS_TEXTURE_DATA: usize = 67;
const FINISHED: usize = 68;
const REPEATED: usize = 69;
pub(super) const TEXTURE_CREATED: usize = 62;

pub(super) fn managed(index: usize) -> bool {
    abi::rt::callbacks::is_set(index)
}

#[derive(Default)]
struct Audio {
    stream: Option<Arc<Stream>>,
    handle: Option<(managed_stream::Ref, HandleId<class::CAudioStreamManaged>)>,
}

pub(super) struct Managed {
    owner: ManagedObject,
    audio: Mutex<Audio>,
}

struct Sink(Arc<Stream>);

impl AudioSink for Sink {
    fn free_frames(&self) -> usize {
        self.0.free_frames()
    }
    fn write(&self, s: &[i16], channels: usize) -> usize {
        self.0.write(s, s.len() / channels.max(1), channels)
    }
    fn clear(&self) {
        self.0.clear();
    }
    fn set_paused(&self, paused: bool) {
        self.0.set_paused(paused);
    }
}

impl Managed {
    pub(super) fn new(owner: ManagedObject) -> Self {
        Self { owner, audio: Mutex::default() }
    }

    /// `GetAudioStream`: the handle over the track's stream, made on first ask; 0 without one.
    pub(super) fn audio_handle(&self) -> HandleId<class::CAudioStreamManaged> {
        let mut a = self.audio.lock().unwrap();
        let Some(stream) = a.stream.clone() else { return HandleId::new(0) };
        HandleId::new(a.handle.get_or_insert_with(|| managed_stream::adopt(stream)).1.id)
    }

    /// The track is gone (stop, a new file, destroy): close the stream and free its handle.
    /// `notify` tells managed to drop its SoundHandle.
    pub(super) fn free_audio(&self, notify: bool) {
        let mut a = self.audio.lock().unwrap();
        let had = a.stream.take().inspect(|s| s.close()).is_some();
        if let Some((r, _)) = a.handle.take() {
            managed_stream::destroy(r);
        }
        if had && notify && managed(FREE_AUDIO) {
            cb::on_free_audio_internal(self.owner);
        }
    }

    pub(super) fn has_audio(&self) -> bool {
        self.audio.lock().unwrap().stream.is_some()
    }
}

impl Events for Managed {
    fn audio(&self, rate: u32, channels: u32) -> Option<Arc<dyn AudioSink>> {
        let stream = Arc::new(Stream::new(channels as usize, rate));
        self.audio.lock().unwrap().stream = Some(stream.clone());
        if managed(INIT_AUDIO) {
            cb::on_init_audio_internal(self.owner, rate as i32, channels as i32);
        }
        Some(Arc::new(Sink(stream)))
    }

    fn loaded(&self, _: &Info) {
        if managed(LOADED) {
            cb::on_loaded_internal(self.owner);
        }
    }

    fn finished(&self) {
        if managed(FINISHED) {
            cb::on_finished_internal(self.owner);
        }
    }

    fn repeated(&self) {
        if managed(REPEATED) {
            cb::on_repeated_internal(self.owner);
        }
    }

    fn warn(&self, message: &str) {
        abi::rt::log::warn(message);
    }
}
