# Audio

Native's half of s&box's sound system (closed `soundsystem` + `steamaudio`). Code: `crates/audio`
(no abi types, unit-tested) and the slot bindings in `crates/engine2/src/audio/`.

## Who does what

Managed mixes. `Sandbox.Audio.MixingThread` builds a voice snapshot each frame and, per 512-sample
block (`MIX_BUFFER_SIZE`, 44100 Hz, about 11.6 ms), runs the mixer graph: `Mixer.Master` and its
children, voices, processors, the master limiter. Volume, distance falloff, panning and the
listener graph are all managed. Native supplies:

| Native piece | Slots | What it does |
| --- | --- | --- |
| Mix buffers | `CAudioMixBuffer`, `CAudioMixDeviceBuffers` | 512 f32 blocks managed writes through a raw pointer (atomics on our side) |
| Sounds | `g_pSoundSystemInternal`, `CSfxTable`, `VSound_t` | load a `.vsnd` or raw PCM into decoded, planar f32 `Pcm` |
| Samplers | `CAudioMixer` | read one block per call from a sound or stream at `rate/44100 * pitch`, linear interpolation, loops, delay (counted down at that pitched rate, as real does), sample end |
| Streams | `CAudioStreamManaged` | i16 queue game code (and voice chat) writes, a sampler drains |
| Effects | `CDirectEffect`, `CBinauralEffect`, `CReverbEffect`, `CPathEffect`, `CAudioProcessor` | Steam Audio's binaural effect itself; direct/reverb stand-ins, delay and pitch shift |
| DSP presets | `DspPreset`, `DspInstance` | Source-style chains (reverb, diffusor, delay, filter, amp) built by `DspFactory` |
| Output | `g_pAudioDevice` | interleaves each `SendOutput` block into a queue a device drains |
| Voice | `VoiceGlue`, `globalOVRLipSync` | Opus 48 kHz mono 20 ms frames (libopus), capture, a viseme estimator |
| Decoders | `CSimplePCMWaveData` | MP3/OGG to PCM for `SoundData.FromMP3`/`FromOGG` |

## Per frame

```
main thread: MixingThread.UpdateGlobals (snapshot)            AudioEngine.Tick -> MuteDevice
feeder thread (native): while queue < mix-ahead: callback MixOneBuffer
    managed: AudioSampler.Sample -> CAudioMixer.ReadToBuffer (native resample)
             Mixer graph -> CDirectEffect / CBinauralEffect / CReverbEffect / processors
             MultiChannelBuffer.SendToOutput -> g_pAudioDevice.SendOutput (native queue)
device thread (cpal or a clock): pulls interleaved f32 from the queue; if it's short, calls
    MixOneBuffer itself first; silence only if managed sends nothing
```

Native pulls; managed never pushes on its own. The feeder starts on the first `MuteDevice`
(managed: "calling MuteDevice actually starts the audio system"). Mix-ahead is 4 blocks, or
`SetMixAhead` seconds while loading. `MixingThread`'s contract is that the feeder mixes ahead and
the device callback mixes "when it falls behind" (real engine2 drives an
`SDL_OpenAudioDeviceStream`), so `Queue::pull` mixes on the device's thread until it has what it
was asked for. Managed's mix lock serializes the two callers. A device thread that stalls and then
catches up therefore plays sound, not silence. The device swap (`SetActiveAudioDevice`) stops both
callers before it takes `DEVICE`'s write lock, because a mix's `SendOutput` reads `DEVICE`.

Underruns (short pulls after the first block) are counted on the queue; engine2 logs a warning
(`audio device ... underran`) from the next `SendOutput` and `OutputDebugInfo` prints the totals.

The disk device writes its file on its own thread. Written from the clock, a busy disk blocked
playback for 20 to 450 ms a write, and the catch-up then drained the queue faster than the feeder
mixed. Those were the underruns in docs/differential-audio.md. Managed's mix never stalled outside
loading: its slowest blocks, 15 to 31 ms, all came during loading, under the 0.5 s loading mix-ahead.

## Devices

- `dedicated_server` or `test_mode` app, or `UNDERBOX_AUDIO=off`: no device. `IsValid` is false, so
  managed never mixes or calls `SendOutput`; nothing is opened.
- `UNDERBOX_AUDIO=null` (or `SDL_AUDIO_DRIVER=dummy`): the null device, a clock thread that drains
  the queue in real time. Same mixing path as a real device, with nothing to hear.
- `UNDERBOX_AUDIO=disk` (or `SDL_AUDIO_DRIVER=disk`): the null clock, appending what it plays to
  `SDL_AUDIO_DISK_OUTPUT_FILE` as interleaved f32 LE, as SDL's disk driver does for real engine2
  (docs/differential-audio.md).
- Otherwise cpal's default output (feature `audio-device` in engine2, on by default), falling back to
  no device if none opens. The device runs at 44100 Hz when it can; otherwise the queue is resampled.

Tests drive `audio::output::Queue` directly: push blocks, pull frames, compare buffers; a slow
mixer and a blocking file stand in for managed and a busy disk (`crates/audio/src/output/tests.rs`).
End to end: `tools/diff/run.sh sound ours` fails on any underrun in the capture.

## Sounds

`PrecacheSound("x.vsnd")` resolves through the native filesystem: a source file next to it
(`x.wav`, `.mp3`, `.ogg`, `.flac`) decoded by symphonia, else the compiled `x.vsnd_c` (header + trailing
stream, keeping its loop points; layout in research/resources.md). `CreateSound` takes managed's PCM: format 0
PCM16, 1 PCM8, 3 f32 (managed decodes ADPCM itself). Everything is decoded once into planar f32.

## Effects (as managed defines them)

- Binaural: Steam Audio's own binaural effect and default HRTF (CIPIC subject 124), which real
  engine2 links statically (its HRTF loader's messages are in `libengine2.so`). `crates/steamaudio-sys`
  compiles the part of Steam Audio's core it needs (third_party/steam-audio v4.8.1, PFFFT for the
  FFT, no SOFA) behind a C shim. The HRTF is loaded with RMS normalization (each HRIR scaled to
  the loudness of the one straight ahead), as real's levels show: without it sources to the side
  are 1.2 to 2.7 dB louder in both ears. Directions go from s&box's listener space (+x ahead, +y left, +z
  up) to Steam Audio's (+x right, +y up, −z ahead); `useNearestInterpolation` picks nearest or
  bilinear lookups. Mono input with a nearest lookup ignores the spatial blend (Steam Audio only
  blends stereo there), so `ListenLocal` voices get the full on-axis HRTF: about −3.4/−3.3 dB at
  440 Hz, the level drop real shows on 2D sounds. Without the HRTF, `Create` returns null and
  managed falls back to its pan law.
- Direct: per band gain `distance * air[b] * (occlusion + (1 - occlusion) * transmission[b])` over
  Steam Audio's bands (< 800 Hz, 800 Hz to 8 kHz, > 8 kHz), ramped across the block.
- Reverb: a 4-line feedback delay network whose line gains give the mid band's T60; the high
  band's T60 sets a damping filter. TODO: the low band.
- Path (`CPathEffect`): passes the input through. Managed never creates one yet.
- DSP presets: per-unit approximations of Source's processors; delay-time modulation is a TODO.

## Voice

`VoiceManager`: 48 kHz mono, 20 ms frames, 32 kbit/s, packets of `[u16 length][opus]`. Encode
takes f32, decode gives i16, so Opus packets interoperate with real s&box clients. Capture is
cpal's input device; without one `OpenCapture` returns null and managed gives up on the mic.
