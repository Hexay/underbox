# Video

`CVideoPlayer` and `CVideoRecorder` (managed `VideoPlayer`, `MusicPlayer`, `VideoTextureLoader`,
`VideoWriter`, `ScreenRecorder`). The `video` crate (`crates/video`) holds containers, codecs, the
playback worker and the recorder; `crates/engine2/src/video/` binds the slots, the texture and the
audio stream.

## What managed relies on

| Slot / callback | Contract | Ours |
| --- | --- | --- |
| `Play(url, ext)` | managed passes an http(s) URL or `BaseFileSystem.GetFullPath`'s OS path; playback restarts, unpaused | absolute paths are read from disk, relative ones through the native filesystem (`GAME`), both on the worker thread. URLs are refused with one warning (no HTTP client) |
| `OnLoadedInternal` | once the file is open; managed queues it to its main thread | from the worker, after demuxing |
| `GetDuration`, `GetPlaybackTime`, `Seek`, `Pause`, `Resume`, `IsPaused`, `Get/SetRepeat`, `IsMuted/SetMuted` | seconds; repeat loops (`OnRepeatedInternal`), the end without it fires `OnFinishedInternal` and holds at the duration | a wall clock that runs once loaded; seeks decode from the keyframe at or before the target |
| `Update` (managed `Present`) | shows the frame due now | the newest due frame goes into the texture; the first one makes it (RGBA8888, the frame's size) and calls `OnTextureCreatedInternal`, which `CopyFrom`s `GetTexture`. With `WantsTextureData` the pixels go to `OnTextureDataInternal` instead |
| `GetWidth`/`GetHeight` | picture size | the texture's, else the container's |
| `OnInitAudioInternal`, `GetAudioStream`, `OnFreeAudioInternal` | managed plays the returned `SoundStream` on its main thread | the worker writes PCM into an `audio::stream::Stream`; `GetAudioStream` wraps it in a `CAudioStreamManaged` handle on first ask (main thread). Muted writes silence |
| `SetVideoOnly` | `VideoTextureLoader`: no audio | no audio track is decoded |
| `GetSpectrum`, `GetAmplitude` | 512 FFT magnitudes, loudness (`MusicPlayer`) | from the last 1024 mono samples written |
| `GetMetadata(key)` | tag lookup | Matroska title and tags, lowercased keys; MP4 tags aren't read |
| decode stats, queue depths | overlay counters | worker totals |

A dedicated server has no GPU device (`RenderDeviceApi::Empty`): the worker decodes no video, but
the clock, duration, repeat, finish and audio track behave the same.

Not done: HTTP(S) URLs, MP4 metadata, VP8/VP9/HEVC, Vorbis, multichannel (>2) Opus, 10-bit AV1,
files read whole into memory (fine for game clips, not for long films).

## Codecs and crates

| Need | Crate | Licence |
| --- | --- | --- |
| AV1 | `rav1d` 1.1 (Rust port of dav1d), `bitdepth_8`, no asm (no nasm on CI) | BSD-2-Clause |
| H.264 decode and encode | `openh264` 0.9, `source` feature: Cisco's C++ built by `cc`, asm off without nasm | BSD-2-Clause |
| MP4 read | `re_mp4` 0.5 (AV1, H.264, AAC tracks) | MIT |
| MP4 write | `mp4` 0.14 | MIT |
| Matroska/WebM | `matroska-demuxer` 0.8 | Zlib/MIT/Apache-2.0 |
| Opus | `opus` (libopus from source, already used by voice) | BSD-3-Clause |
| AAC-LC | `symphonia` `aac` (already used for sound files) | MPL-2.0 |

Notices are in NOTICE. OpenH264 built from source carries no H.264 patent licence: Cisco's
royalty cover applies only to the binaries Cisco ships, and fetching those at runtime was ruled
out. Facepunch dropped H.264 from `VideoWriter` for the same reason.

rav1d and openh264-sys2 build at `opt-level = 3` in dev (root `Cargo.toml`); unoptimised they take
seconds per frame.

## Threads

`video::Player` spawns one worker per file: demux the whole file into a packet list (decode
order), then decode ahead of the clock. Video waits while 4 frames are queued; frames already
superseded by the clock are dropped, so a texture nobody presents doesn't stall audio. Audio runs
at most 0.3 s ahead. `rav1d` uses 2 threads of its own. Managed callbacks are never made while the
player's state lock is held: `OnTextureCreatedInternal` calls straight back into `GetTexture`.

## Recorder

`VideoWriter`/`ScreenRecorder` ask for AV1 or VP9 in MP4 or WebM. There is no AV1/VP9 encoder in
the tree (rav1e is far from real time without asm), so the recorder writes H.264 (openh264) in
MP4 for `mp4` and logs the substitution; WebM and WebP fail `Initialize`. Frames are encoded on
their own thread, `ReadyForVideoFrame` is false while 4 wait. Audio (`AddAudioSamples`) is
dropped: there's no AAC encoder or Opus-in-MP4 writer yet.

## Tests and fixtures

`tools/test.sh video` (containers, codecs, YUV, spectrum, the worker's clock, pause, seek, repeat,
no-decoder mode) and `tools/test.sh engine2 video` (frames reach the texture, slot state, recorder
round trip). Clips are synthetic (`video::fixture`): H.264 and Opus are encoded in-test, Matroska
is written by `fixture::matroska`, MP4 by the recorder. AV1 has no in-tree encoder, so
`crates/video/src/fixture/av1-64x64.ivf` (1 KB) is checked in: 16 frames of `fixture::frame`'s
palette bands, 64x64, 8 fps, keyframe every 4, made once with rav1e 0.8 (speed 10, quantizer 40,
BT.601) by a throwaway program outside the repo.

Golden frames `frame-video` and `frame-video-managed` (`CORPUS=video tools/client-frame.sh`): a
UI panel whose `background-image` is `videos/still.webm`, so it goes through managed's
`VideoTextureLoader` (repeat, `Present` every tick) into our texture. The clip loops one still
(four palette quadrants, H.264 in a `.webm`-named Matroska file) so the frame settles; rewrite it
with `tools/test.sh video write_corpus_clip -- --ignored`.
