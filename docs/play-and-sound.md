# Playing real games: window input and sound

Part of the real-games runs (docs/real-games.md): `tools/game.sh` `MODE=play` and `MODE=sound`.

## Playing in a window

`MODE=play` runs the client in a real SDL window on X11, on the virtual display `xvfb-run` makes,
and drives it with XTest. `tools/game/xinput.py` calls libXtst through ctypes, so no xdotool is
needed, and `tools/game/play.sh` runs the steps once the live client log matches `PLAY_TRIGGER`.
The default steps: point at the window and give it focus (there is no window manager), hold W for
6 s, move the mouse 600 px right over 4 s, then save the screen as X shows it (`$OUT/screen.png`).
`PLAY_STEPS` overrides them. The watcher logs `GAME:INPUT` when managed first sees input. On the
server it logs `GAME:PLAYER_MOVED`/`PLAYER_TURNED` when a stood player moves 32 units or turns
10°. On a client, `GAME:CAMERA_MOVED`/`CAMERA_TURNED` are measured from the camera's pose at that
first input. `PLAY_VIA=host` plays a host client instead of joining a dedicated server.

Zoo, joined client (`MODE=play`, ~2 min):

```
PASS client window on X11: window: SDL video driver x11
PASS managed saw input: GAME:INPUT move=1,0,0 ... mouse_visible=False
PASS client view turned: GAME:CAMERA_TURNED view yaw=0->-15
PASS server sees the player move: GAME:PLAYER_MOVED owner=Player local=False dist=49 from=12,-1 to=60,-1
PASS server sees the player turn: GAME:PLAYER_TURNED owner=Player local=False yaw=0->-19
```

**Fixed:** mouse look. Our SDL is built without XInput2: `crates/window/build.rs` turns each X11
extension off when its header is missing, and this host has no `libxi-dev`. SDL's X11 backend only
does relative mode through XInput2, so `SetWindowRelativeMouseMode` failed with "That operation is
not supported". Managed reads mouse deltas only in relative mode (`SdlEvents.cs`), so the view
never turned. `crates/window/src/relative.rs` now emulates relative mode, as SDL2's warp mode
did: it hides the cursor, reports each motion as its offset from the window centre, warps back,
and drops the warp's own event. `crates/window/tests/relative.rs` covers it. With `libxi-dev`
installed at build time, SDL's own relative mode takes over. Keyboard input needed no fix.

SDL in this build also lacks XRandR, XFixes, XCursor, XTest and XScrnSaver for the same reason.
Install `libxi-dev libxrandr-dev libxfixes-dev libxcursor-dev libxtst-dev libxss-dev` and
rebuild to get them. Xvfb is already installed (`xvfb-run`).

The X screenshot shows Sweeper's cube blue, as its tint `0.11,0.31,0.86` says, and the window
presents correctly. The `UNDERBOX_FRAMES` dump of the same window (`frame.png`) has red and blue
swapped: the cube comes out brown. On the headless offscreen swap chain the dump's colours are
right. Handed to the renderer lane. The likely cause is that the dump reads the X11 swap chain's
BGRA as RGBA. That hasn't been checked.

## Sound

`UNDERBOX_AUDIO_STATS=1` (always on in `tools/game.sh`) logs, every ~5 s of mixed audio: voices
started and failed (`CreateMixer` with and without samples), the blocks samplers read, the blocks
sent to the device, each split by whether it carried signal, and the peak. It also names the first
12 sounds that started and the first 12 that failed (`crates/engine2/src/audio/stats.rs`). Native
lines go to the client's stdout, `client.out`.

| Run | Result |
|---|---|
| Sandbox host, `START_CMD='spawn prop:models/dev/box.vmdl'` | `voice started: sounds/ui/ui.spawn.1.vsnd` (the checkout's mp3). `voices=1 voice_blocks=28 audible_voice_blocks=19 audible_output_blocks=20 peak=0.133` |
| Grubs round (`MODE=sound START_CMD=gr_ffa_next_state`) | `voices=1 voice_blocks=5938 audible_voice_blocks=5938 audible_output_blocks=5938 peak=0.018` |
| Zoo idle, joined client, samples | `voices=0`. Every sound they ask for is a package sound (weapons, physgun), menu music (`.meta` only) or, on a joined client, game content that isn't downloaded |

`voice_failures` counts sounds with no samples, retried each time they're played (135 in the zoo
run). Each one is logged once as "sound ...: no source or compiled file".

**Fixed:** headless clients mixed about 15x faster than real time. `tools/boot-client.sh` points
ALSA's default at the `null` PCM, which accepts samples as fast as they come, so cpal's callback
had managed run `MixOneBuffer` flat out. Sweeper logged 298,850 output blocks (about 58 min of
audio) in a 4-minute run. `boot-client.sh` now defaults to `UNDERBOX_AUDIO=null`, engine2's null
device, whose clock drains in real time. Same zoo join: 48.6 s of client time on the null device,
against 58.6 s and 25,370 blocks on ALSA's null PCM. `UNDERBOX_AUDIO=device` brings cpal back.
