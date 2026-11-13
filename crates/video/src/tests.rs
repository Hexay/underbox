//! Container and codec round trips over the synthetic fixtures.

use crate::decode::{AudioDecoder, Frame, VideoDecoder};
use crate::demux::{self, Kind, Media, VideoCodec};
use crate::fixture::{self, PALETTE, near, pixel};

fn decode_all(m: &Media) -> Vec<Frame> {
    let mut dec = VideoDecoder::new(&m.video.as_ref().unwrap().codec).unwrap();
    let mut out = Vec::new();
    for p in m.packets.iter().filter(|p| p.kind == Kind::Video) {
        dec.decode(m.data(p), p.pts, &mut out).unwrap();
    }
    dec.flush(&mut out);
    out
}

/// Frame `i` of a fixture, checked by its two bands.
fn assert_shows(f: &Frame, i: usize) {
    let (top, bottom) = (pixel(&f.rgba, f.width, f.width / 2, f.height / 4), pixel(&f.rgba, f.width, f.width / 2, f.height * 3 / 4));
    assert!(near(top, PALETTE[i % 8], 24) && near(bottom, PALETTE[(i + 4) % 8], 24), "frame {i} at {}: {top:?} {bottom:?}", f.pts);
}

#[test]
fn h264_in_matroska_demuxes_and_decodes_every_frame() {
    let m = demux::open(fixture::h264_mkv(64, 48, 30, 10, true)).unwrap();
    let v = m.video.as_ref().unwrap();
    assert!(matches!(v.codec, VideoCodec::H264(_)) && (v.width, v.height) == (64, 48));
    let a = m.audio.as_ref().unwrap();
    assert_eq!((a.rate, a.channels), (48000, 2));
    assert_eq!(m.packets.iter().filter(|p| p.kind == Kind::Video).count(), 10);
    assert!((m.duration - 10.0 / 30.0).abs() < 0.06, "{}", m.duration);
    assert_eq!(m.metadata, [("title".to_string(), "Synthetic".to_string())]);
    let frames = decode_all(&m);
    assert_eq!(frames.len(), 10);
    for (i, f) in frames.iter().enumerate() {
        assert!((f.pts - i as f64 / 30.0).abs() < 0.002);
        assert_shows(f, i);
    }
}

#[test]
fn h264_in_mp4_from_the_recorder_plays_back() {
    let path = std::env::temp_dir().join(format!("underbox-video-{}.mp4", std::process::id()));
    fixture::h264_mp4(&path, 96, 64, 24, 12).unwrap();
    let m = demux::open(std::fs::read(&path).unwrap()).unwrap();
    std::fs::remove_file(&path).ok();
    assert!((m.duration - 0.5).abs() < 0.05, "{}", m.duration);
    assert!(m.audio.is_none() && m.packets[0].keyframe);
    let frames = decode_all(&m);
    assert_eq!(frames.len(), 12);
    frames.iter().enumerate().for_each(|(i, f)| assert_shows(f, i));
}

#[test]
fn av1_in_webm_decodes_every_frame() {
    let m = demux::open(fixture::av1_webm(false)).unwrap();
    let (w, h, fps, n) = fixture::AV1;
    assert!(matches!(m.video.as_ref().unwrap().codec, VideoCodec::Av1));
    let frames = decode_all(&m);
    assert_eq!(frames.len(), n);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!((f.width, f.height), (w, h));
        assert!((f.pts - i as f64 / fps as f64).abs() < 0.002);
        assert_shows(f, i);
    }
}

#[test]
fn opus_decodes_to_its_length_after_pre_skip() {
    let m = demux::open(fixture::av1_webm(true)).unwrap();
    let mut dec = AudioDecoder::new(m.audio.as_ref().unwrap()).unwrap();
    let mut pcm = Vec::new();
    for p in m.packets.iter().filter(|p| p.kind == Kind::Audio) {
        dec.decode(m.data(p), &mut pcm).unwrap();
    }
    let seconds = fixture::AV1.3 as f64 / fixture::AV1.2 as f64;
    let frames = pcm.len() / 2;
    assert!(frames < (seconds * 48000.0) as usize && frames > (seconds * 48000.0) as usize - 1000, "{frames}");
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!((12000..20000).contains(&peak), "{peak}");
}

#[test]
fn seeking_starts_at_the_keyframe_before() {
    let m = demux::open(fixture::av1_webm(true)).unwrap();
    let i = m.seek_index(0.9, true);
    let p = &m.packets[i];
    assert!(p.kind == Kind::Video && p.keyframe && (p.pts - 0.5).abs() < 1e-6, "{p:?}");
    let audio_only = m.seek_index(0.9, false);
    assert!(m.packets[audio_only].pts >= 0.9 - 1e-6 && m.packets[audio_only - 1].pts < 0.9);
}

const CORPUS_CLIP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/video/Assets/videos/still.webm");

/// Rewrites corpus/video's clip: `tools/test.sh video write_corpus_clip -- --ignored`.
#[test]
#[ignore]
fn write_corpus_clip() {
    std::fs::write(CORPUS_CLIP, fixture::still_webm(320, 180)).unwrap();
}

#[test]
fn corpus_clip_is_a_still() {
    let Ok(bytes) = std::fs::read(CORPUS_CLIP) else { return };
    let frames = decode_all(&demux::open(bytes).unwrap());
    assert_eq!(frames.len(), 10);
    assert!(frames.iter().all(|f| (f.width, f.height) == (320, 180)));
    let f = &frames[9];
    let quadrants = [(80, 45, 0), (240, 45, 1), (80, 135, 2), (240, 135, 3)];
    assert!(quadrants.iter().all(|&(x, y, c)| near(pixel(&f.rgba, f.width, x, y), PALETTE[c], 24)));
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(demux::open(b"not a video".to_vec()).is_err());
    assert!(demux::open([0x1A, 0x45, 0xDF, 0xA3, 0x80].to_vec()).is_err());
}
