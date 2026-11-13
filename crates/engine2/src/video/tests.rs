//! The slots' contract with managed, over synthetic clips (`video::fixture`).

use super::player::{self, Ref};
use super::recorder;
use crate::render::{device, texture};
use abi::ManagedObject;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use video::fixture::{self, PALETTE, near, pixel};

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("underbox-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn duration(p: Ref) -> f64 {
    player::playing(p, |v| v.info().map_or(0.0, |i| i.duration))
}

#[test]
fn video_frames_reach_the_texture_managed_samples() {
    if device::create().is_none() {
        return eprintln!("skipped: no GPU");
    }
    let path = temp("frames.mkv", &fixture::h264_mkv(64, 32, 30, 30, true));
    let p = player::create(ManagedObject(1));
    player::with(p, |s| s.repeat = true);
    assert!(player::play(p, path.to_str().unwrap()));
    until("loaded", || duration(p) > 0.0);
    assert_eq!(player::size(p), (64, 32));
    until("a texture", || {
        player::update(p);
        player::with(p, |s| s.texture.is_some())
    });
    let (t, w, h) = player::with(p, |s| s.texture).unwrap();
    assert_eq!((w, h), (64, 32));
    let cpu = texture::resolve(t).unwrap().cpu_mip0().unwrap();
    let top = pixel(&cpu.pixels, w, 32, 8);
    assert!(PALETTE.iter().any(|c| near(top, *c, 24)), "{top:?}");
    assert!(player::with(p, |s| s.events.as_ref().unwrap().has_audio()));
    player::stop(p, false);
    assert!(player::with(p, |s| s.playing.is_none() && s.texture.is_some()));
    assert!(objects::destroy(p));
    std::fs::remove_file(path).ok();
}

#[test]
fn state_slots_hold_before_and_after_play() {
    let path = temp("state.webm", &fixture::av1_webm(false));
    let p = player::create(ManagedObject(2));
    player::with(p, |s| (s.muted, s.paused) = (true, true));
    assert_eq!(duration(p), 0.0);
    assert!(!player::play(p, "https://example.com/a.mp4"), "URLs aren't fetched");
    assert!(!player::play(p, "/no/such/file.mp4"));
    assert!(player::play(p, path.to_str().unwrap()));
    assert!(player::with(p, |s| !s.paused && s.muted), "Play unpauses and keeps mute");
    until("loaded", || duration(p) > 0.0);
    assert!((duration(p) - 2.0).abs() < 0.1, "{}", duration(p));
    player::playing(p, |v| v.pause());
    player::playing(p, |v| v.seek(1.5));
    assert_eq!(player::playing(p, |v| v.time()), 1.5);
    assert!(objects::destroy(p));
    assert_eq!(duration(p), 0.0, "a destroyed player reads as default");
    std::fs::remove_file(path).ok();
}

#[test]
fn recorder_writes_an_mp4_that_plays_back() {
    let path = std::env::temp_dir().join(format!("underbox-{}-rec.mp4", std::process::id()));
    let r = recorder::create();
    let config = video::recorder::Config { width: 32, height: 32, fps: 10, bitrate_mbps: 1 };
    assert!(!recorder::initialize(r, path.to_str().unwrap(), config, "vp9", "webm"));
    assert!(recorder::initialize(r, path.to_str().unwrap(), config, "av1", "mp4"));
    for i in 0..5 {
        recorder::add(r, &fixture::frame(32, 32, i), i as i64 * 100_000);
    }
    recorder::finish(r);
    let media = video::demux::open(std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(media.packets.len(), 5);
    assert!((media.duration - 0.5).abs() < 0.05, "{}", media.duration);
    objects::destroy(r);
    std::fs::remove_file(path).ok();
}
