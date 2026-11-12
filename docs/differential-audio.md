# Audio differential

Compare the final mixed audio of our engine2 and Facepunch's on one scripted scene
(docs/differential.md has the rules and the other lanes). **Status: runs on both engines** with
`tools/diff/lavapipe.sh` built once ([Getting real to mix](#getting-real-to-mix)); every level
matches, see [Real vs ours](#real-vs-ours).

## Harness

| Piece | What |
| --- | --- |
| `tools/diff/run.sh sound [both\|ours\|real]` | Runs `tools/diff/sound-run.sh`: corpus/sound on each engine through `tools/boot-client.sh`, captures, compares. Out: `target/diff/sound/` |
| `corpus/sound` | Camera at the origin facing +x, `SoundScript` (`Code/SoundScript.cs`) |
| `tools/diff/sound.py <ours.raw> [<real.raw>]` | Measures each capture; with one, fails on underruns; with two, ranks the divergences |
| `fixture::tone_files` (`crates/assets/src/compiled/fixture/tones.rs`) | The sounds, compiled `.vsnd_c` (v4 binary header, the layout real's own v5 files use), committed under `corpus/sound/Assets/sounds/compiled/`; `UNDERBOX_BLESS=1` rewrites them |

### Capture

Real opens audio through its SDL3 (`SDL_OpenAudioDeviceStream`; there is no other audio library in
its process), and that SDL3 has the `disk` driver. So both engines take SDL's contract:
`SDL_AUDIO_DRIVER=disk SDL_AUDIO_DISK_OUTPUT_FILE=<file>`. Real writes interleaved f32 LE stereo at
44100 Hz (checked from its file size against wall time). Ours honours the same variables in
`audio::device::mode` (`UNDERBOX_AUDIO=disk` takes precedence over `UNDERBOX_AUDIO=null`) and writes
the same format from a clock that drains one block per block of wall time
(`audio::output::spawn_disk_clock`). Both record what the device played, underruns included.

### Scene

`SoundScript` loads the sounds in `OnStart`, waits until they are playable (`LoadAsync`), then a
second more (so the 0.5 s loading mix-ahead has drained), then makes every `PlayFile` call at once with a `delay`, so onsets are
sample-accurate and don't depend on frame timing. Real loads sound data asynchronously, and
`PlayFile` drops the delay while the sound has no mixer yet (`SoundHandle.sampler` null), so
without the wait every sound started together. Only the loop's stop is timed by the main thread.
Sounds: mono PCM16 at 44100 Hz, peak 0.5, 5 ms ramps.

| s | Event | Sound | Set up |
| --- | --- | --- | --- |
| 0 | `ui` | 440 Hz, 0.5 s | `ListenLocal`, `SpacialBlend` 0, no distance attenuation |
| 1 | `left200` | same | 200 units +y (left) |
| 2 | `right200` | same | 200 units −y |
| 3 | `ahead200` | same | 200 units +x |
| 4 | `ahead800` | same | 800 units +x |
| 5 | `vol0.5` | same | 2D, volume 0.5 |
| 6 | `pitch1.5` | same | 2D, pitch 1.5 (660 Hz, 0.33 s); `delay` 9 s, which the sampler counts down at 1.5x |
| 7 | `noise` | LCG white noise, 0.3 s | 2D |
| 8 | `loop` | 330 Hz, 1 s, loops | 2D, stopped 9.5 s after the start |

### Metrics

Each capture is cut into bursts of sound (joined across gaps under 0.3 s). Each event takes the
burst nearest where it's due after the previous event's onset (within 0.5 s, else `missing`), so an
underrun between two sounds delays one onset instead of shifting every later window. Per event: onset error against the previous event's onset and the schedule, RMS per
channel over the steady part, L/R balance, frequency from interpolated zero crossings, the tail's
level after the sound (reverb), runs of exact zeros inside the sound (dropouts, i.e. underruns),
and the loop's audible length. Divergences are ranked by `|ours − real|` over a per-metric unit
(1 dB, 2 Hz, 5 ms, 30 ms of loop length, one dropout).

## Why real stays silent

Real's capture works; the file is all zeros because its managed mixer never starts.

1. `EngineLoop.FrameStart` calls `IGameInstanceDll.Tick`, which polls rich presence. Under
   `-nosteam` `Steam.SteamFriends()` is null, so `SetRichPresence` throws every frame and the rest
   of the frame is skipped: `SimulateUI`, `MixingThread.UpdateGlobals` and `AudioEngine.Tick`, whose
   first `MuteDevice` starts mixing. So SDL plays silence. (The same exception is why real's hud
   frame draws no UI, docs/differential-render.md.)
2. `UNDERBOX_STANDIN_FRIENDS=1` gives the Steam stand-in a friends interface
   (`tools/diff/standin/friends.cpp`; the game-server context has none). Rich presence passes, the
   UI ticks, and `RootPanel.UpdateScale` crashes in `SteamAPI_ISteamUtils_IsSteamRunningOnSteamDeck`
   on a null `ISteamUtils`: managed P/Invokes the flat accessor `SteamAPI_SteamUtils_v010` through
   its own `dlopen` handle, and libsteam_api binds it internally (no PLT), so the preload can't
   interpose it.
3. The same variable makes `standin.sh --shim` put `standin/flat.cpp` in the run copy as
   `libsteam_api.so` over the renamed original, answering `SteamAPI_SteamUtils_v010` and
   `SteamAPI_SteamFriends_v018` from the stand-in. That gets past it, and the menu UI now runs. It
   calls sbox.game's web API, which the harness must not reach: `tools/diff/real-env.sh` points
   real clients' HTTP(S) proxy at a closed local port.
4. Real then segfaulted about 15 s in, drawing the menu UI, inside lavapipe's JIT'd shader code.
   That was a Mesa 26.0 bug, fixed by a newer lavapipe (docs/differential.md, "Real's UI on
   lavapipe"). With `tools/diff/lavapipe.sh` built, real loads the game and mixes.

Also ruled out: the game-server context's `SteamFriends018` (null); a network namespace to cut the
web API (`unshare -rn` is not permitted here); skipping the menu (no switch for it).

## Real vs ours

`run.sh sound both` (`target/diff/sound/report.txt`). No dropouts on either, every onset within
1 ms of the schedule. Real's figures are the same in every run; "before" is ours with managed's
pan law and a delay counted at the unpitched rate.

| Event | Ours before L / R dB | Ours now = real L / R dB | Hz |
| --- | --- | --- | --- |
| `ui` | −9.19 / −9.19 | −12.50 / −12.33 | 440.0 |
| `left200` | −9.03 / −89.03 | −9.35 / −13.97 | 440.0 |
| `right200` | −89.03 / −9.03 | −13.95 / −9.01 | 440.0 |
| `ahead200` | −9.03 / −9.03 | −12.50 / −12.33 | 440.0 |
| `ahead800` | −20.48 / −20.48 (real merged with `pitch1.5`) | −23.95 / −23.78 | 440.0 |
| `vol0.5` | −15.05 / −15.05 | −18.52 / −18.35 | 440.0 |
| `pitch1.5` | −9.03 / −9.03 (real: missing) | −11.65 / −11.30 | 660.0 |
| `noise` | −10.78 / −10.78 | −13.62 / −11.57 | |
| `loop` | −9.03 / −9.03 | −10.65 / −10.76 | 330.0 |

Every level now matches to 0.01 dB; the only divergence `sound.py` reports is the loop's length.

1. **Level and panning: Steam Audio's HRTF.** Real's `CBinauralEffect` is Steam Audio's binaural
   effect with its default HRTF, loaded with RMS normalization (docs/audio.md, Effects). Its
   on-axis gain at 440 Hz is −3.47/−3.30 dB, the drop on `ui`, `ahead200` and `vol0.5`. Managed's
   own gains are the same on both engines: `ListenLocal` voices take the binaural effect with a
   nearest lookup and spatial blend 0.1, which Steam Audio ignores for mono input, so 2D sounds
   get the full on-axis HRTF. A probe of 30° to 135° at 200 and 400 units, with and without
   occlusion, matched within 0.02 dB once RMS normalization was on; without it, ILDs already
   matched and only a gain equal in both ears differed (−1.2 to −2.7 dB).
2. **Distance:** `ahead800` is 11.45 dB under `ahead200` on both (managed's falloff).
3. **Pitch-scaled delay is native:** managed passes `delay * SampleRate` frames to
   `DelayOrSkipSamples`; real's sampler counts them down at the pitched rate. Ours does too now
   (`audio::sampler`), and the scene asks for 9 s to start `pitch1.5` at 6 s.
4. **Loop length is main-thread timing, not native.** Not converged by design of the scene: the
   stop is a frame-timed `Stop()`, so the loop's length is `stop time − 8 s` (logged as
   `SOUND:STOP loop at`) plus the difference in queue latency between `Play` and `Stop`. Over five
   real runs the length was 1424 to 1609 ms (stop logged at 9.504 to 9.572 s); ours 1470 to 1520.
   The ~100 ms short loop came from real finishing its async load after the 1 s lead, so `Play` ran
   at once while the 0.5 s loading mix-ahead was still queued; the lead now counts from load
   completion. A sample-accurate stop would need an API managed doesn't have.
5. Real's menu plays a sound (110 Hz or lower, −48 to −34 dB) about 1 s before `ui`.
   `sound.py` starts the schedule at the first burst with another burst 1 s after it, and finds
   each later event's burst near where it's due, so that sound or a merged slot doesn't shift the
   rest.

## Ours

No event has a reverb tail (−120 dB): nothing in the scene sets up reverb. Earlier captures had
underruns; the cause was our disk device blocking on its file writes, not the managed mixer
(docs/audio.md "Per frame"). `run.sh sound ours` fails on any underrun (a dropout inside a sound, or
an onset more than 2 ms off the schedule).
