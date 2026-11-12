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
| Samplers | `CAudioMixer` | read one block per call from a sound or stream at `rate/44100 * pitch`, linear interpolation, loops, delay, sample end |
| Streams | `CAudioStreamManaged` | i16 queue game code (and voice chat) writes, a sampler drains |
| Effects | `CDirectEffect`, `CBinauralEffect`, `CReverbEffect`, `CPathEffect`, `CAudioProcessor` | Steam Audio's direct/binaural/reverb stand-ins, delay and pitch shift |
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
device thread (cpal or null clock): pulls interleaved f32 from the queue, silence on underrun
```

The feeder starts on the first `MuteDevice` (managed: "calling MuteDevice actually starts the
audio system"). Mix-ahead is 4 blocks, or `SetMixAhead` seconds while loading.

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

Tests drive `audio::output::Queue` directly: push blocks, pull frames, compare buffers.

## Sounds

`PrecacheSound("x.vsnd")` resolves through the native filesystem: a source file next to it
(`x.wav`, `.mp3`, `.ogg`, `.flac`) decoded by symphonia, else the compiled `x.vsnd_c` (header + trailing
stream, keeping its loop points; layout in research/resources.md). `CreateSound` takes managed's PCM: format 0
PCM16, 1 PCM8, 3 f32 (managed decodes ADPCM itself). Everything is decoded once into planar f32.

## Effects (as managed defines them)

- Binaural: managed's own non-Steam-Audio fallback pan law (`BinauralEffect.Apply`): input averaged
  to mono, per-ear gain `(clamp(±left) + fwd)^snd_dir_power` blended by spatial blend, smoothed 0.1
  per block. TODO: HRTF.
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
