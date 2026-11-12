# Audio differential

Compare the final mixed audio of our engine2 and Facepunch's on one scripted scene
(docs/differential.md has the rules and the other lanes). **Status: blocked on real's side.** Real's
output can be captured, but real never mixes a sound in a headless `-nosteam` client; see
[Why real stays silent](#why-real-stays-silent). Ours runs end to end.

## Harness

| Piece | What |
| --- | --- |
| `tools/diff/run.sh sound [both\|ours\|real]` | Runs `tools/diff/sound-run.sh`: corpus/sound on each engine through `tools/boot-client.sh`, captures, compares. Out: `target/diff/sound/` |
| `corpus/sound` | Camera at the origin facing +x, `SoundScript` (`Code/SoundScript.cs`) |
| `tools/diff/sound.py <ours.raw> [<real.raw>]` | Measures each capture; with two, ranks the divergences |
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

`SoundScript` waits a second after `OnStart`, then makes every `PlayFile` call at once with a
`delay`, so onsets are sample-accurate and don't depend on frame timing. Only the loop's stop is
timed by the main thread. Sounds: mono PCM16 at 44100 Hz, peak 0.5, 5 ms ramps.

| s | Event | Sound | Set up |
| --- | --- | --- | --- |
| 0 | `ui` | 440 Hz, 0.5 s | `ListenLocal`, `SpacialBlend` 0, no distance attenuation |
| 1 | `left200` | same | 200 units +y (left) |
| 2 | `right200` | same | 200 units −y |
| 3 | `ahead200` | same | 200 units +x |
| 4 | `ahead800` | same | 800 units +x |
| 5 | `vol0.5` | same | 2D, volume 0.5 |
| 6 | `pitch1.5` | same | 2D, pitch 1.5 (660 Hz, 0.33 s) |
| 7 | `noise` | LCG white noise, 0.3 s | 2D |
| 8 | `loop` | 330 Hz, 1 s, loops | 2D, stopped 9.5 s after the start |

### Metrics

Each capture is cut into bursts of sound (joined across gaps under 0.3 s) and the bursts are matched
to the events in order, so an underrun between two sounds delays one onset instead of shifting every
later window. Per event: onset error against the previous event's onset and the schedule, RMS per
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
   calls sbox.game's web API (401 Unauthorized, and the harness must not talk to it; pointing
   `HTTPS_PROXY` at a closed local port stops that), then real segfaults about 15 s in, before the
   game loads, in lavapipe's JIT'd shader code (AVX-512, a bounds check reading a `{pointer, size}`
   buffer descriptor). It does the same with `LP_NUM_THREADS=0`; one run of five hung instead.
   That is real's UI drawing on llvmpipe, not audio, and nothing in our harness can change it.

Also ruled out: the game-server context's `SteamFriends018` (null); a network namespace to cut the
web API (`unshare -rn` is not permitted here); skipping the menu (no switch for it).

To unblock: a GPU, or real's UI draw on lavapipe fixed in the render lane; or a way to tick
`AudioEngine` without the UI. Then `UNDERBOX_STANDIN_FRIENDS=1` (set by `sound-run.sh`) gets real
mixing.

## Ours

Two runs (`sound.py run1.raw run2.raw`). Levels, balance and frequency are identical in both:

| Event | L / R dB | Hz | Note |
| --- | --- | --- | --- |
| `ui` | −9.19 / −9.19 | 440.0 | 0.16 dB under the 3D sounds, in both runs |
| `left200` | −9.03 / −89.03 | 440.0 | managed's fallback pan law, hard left |
| `right200` | −89.03 / −9.03 | 440.0 | |
| `ahead200` | −9.03 / −9.03 | 440.0 | |
| `ahead800` | −20.48 / −20.48 | 440.0 | 11.45 dB below 200 units |
| `vol0.5` | −15.05 / −15.05 | 440.0 | −6.02 dB |
| `pitch1.5` | −9.03 / −9.03 | 660.0 | |
| `noise` | −10.78 / −10.78 | | |
| `loop` | −9.03 / −9.03 | 330.0 | |

A sine at peak 0.5 is −9.03 dB RMS, so levels, distance, volume and pitch come out as managed
defines them. No event has a reverb tail (−120 dB): nothing in the scene sets up reverb. Onsets
are within 0.6 ms of the schedule, except for underruns: the device ran dry and played silence
for 81 ms before `ahead200` in one run, 836 ms before `right200` in the other, and one block
(11.6 ms) inside the loop in the first. The managed mixer stalled for longer than the 4-block
(46 ms) mix-ahead. The loop's stop is timed on the main thread, so a stall before it shortens the
loop (1439 ms, then 661 ms). Whether real underruns the same way is the first thing to check once
real mixes. Expected divergence then: real's binaural effect is Steam Audio's HRTF
(`snd_steamaudio` on), ours is managed's pan law, so `left200`/`right200` balance and levels will
differ.
