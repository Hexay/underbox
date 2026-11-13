//! `CVideoPlayer` and `CVideoRecorder` slots over the `video` crate (docs/video.md). A dedicated
//! server has no GPU device, so its players decode nothing but still keep time, duration, repeat
//! and the finished/repeated callbacks.

mod events;
mod player;
mod recorder;
#[cfg(test)]
mod tests;

use abi::slots::c_video_player as vp;
use abi::{HandleId, Registry, StrRet, bind};
use player::{playing, with};

fn players(r: &mut Registry) {
    bind!(r, vp::CREATE, player::create);
    bind!(r, vp::DESTROY, |p| {
        objects::destroy(p);
    });
    bind!(r, vp::PLAY, |p, url, _ext| player::play(p, &url.map(|u| u.to_string_lossy().trim().to_string()).unwrap_or_default()));
    bind!(r, vp::STOP, |p| player::stop(p, true));
    bind!(r, vp::RESUME, |p| with(p, |s| {
        s.paused = false;
        s.playing.as_ref().inspect(|v| v.resume());
    }));
    bind!(r, vp::PAUSE, |p| with(p, |s| {
        s.paused = true;
        s.playing.as_ref().inspect(|v| v.pause());
    }));
    bind!(r, vp::SEEK, |p, t| playing(p, |v| v.seek(t)));
    bind!(r, vp::GET_REPEAT, |p| with(p, |s| s.repeat));
    bind!(r, vp::SET_REPEAT, |p, on| with(p, |s| {
        s.repeat = on;
        s.playing.as_ref().inspect(|v| v.set_repeat(on));
    }));
    bind!(r, vp::IS_PAUSED, |p| with(p, |s| s.paused));
    bind!(r, vp::IS_MUTED, |p| with(p, |s| s.muted));
    bind!(r, vp::SET_MUTED, |p, on| with(p, |s| {
        s.muted = on;
        s.playing.as_ref().inspect(|v| v.set_muted(on));
    }));
    bind!(r, vp::SET_VIDEO_ONLY, |p| with(p, |s| s.video_only = true));
    bind!(r, vp::UPDATE, player::update);

    bind!(r, vp::GET_DURATION, |p| playing(p, |v| v.info().map_or(0.0, |i| i.duration as f32)));
    bind!(r, vp::GET_PLAYBACK_TIME, |p| playing(p, |v| v.time() as f32));
    bind!(r, vp::HAS_AUDIO_STREAM, |p| with(p, |s| s.events.as_ref().is_some_and(|e| e.has_audio())));
    bind!(r, vp::GET_WIDTH, |p| player::size(p).0);
    bind!(r, vp::GET_HEIGHT, |p| player::size(p).1);
    bind!(r, vp::GET_METADATA, |p, key| {
        let key = key.map(|k| k.to_string_lossy().into_owned()).unwrap_or_default();
        StrRet::from(playing(p, |v| v.metadata(&key)).unwrap_or_default())
    });
    bind!(r, vp::GET_SPECTRUM, |p, out| {
        let spectrum = playing(p, video::Player::spectrum);
        crate::containers::with(out, |v: &mut Vec<f32>| *v = spectrum);
    });
    bind!(r, vp::GET_AMPLITUDE, |p| playing(p, video::Player::amplitude));
    bind!(r, vp::GET_VIDEO_DECODE_SECONDS, |p| playing(p, |v| v.stats().video_seconds));
    bind!(r, vp::GET_AUDIO_DECODE_SECONDS, |p| playing(p, |v| v.stats().audio_seconds));
    bind!(r, vp::GET_DECODED_FRAME_COUNT, |p| playing(p, |v| v.stats().frames as i32));
    bind!(r, vp::GET_PACKET_QUEUE_DEPTH, |p| playing(p, |v| v.stats().packet_queue as i32));
    bind!(r, vp::GET_FRAME_QUEUE_DEPTH, |p| playing(p, |v| v.stats().frame_queue as i32));
    bind!(r, vp::GET_TEXTURE, |p| {
        let t = with(p, |s| s.texture.map(|t| t.0));
        crate::resource::handed_out::<_, crate::render::texture::Texture>(t)
    });
    bind!(r, vp::GET_AUDIO_STREAM, |p| with(p, |s| s.events.clone()).map_or(HandleId::new(0), |e| e.audio_handle()));
}

pub fn register(r: &mut Registry) {
    players(r);
    recorder::register(r);
}
