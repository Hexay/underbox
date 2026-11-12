# Differential runs against real engine2

The third correctness check in docs/PLAN.md: run the same managed build and the same scripted
scenario on Facepunch's closed engine2 and on ours, and compare what managed code observes. The
user confirmed that the s&box EULA allows this.
Mixed sessions (Facepunch's client on our server and the reverse): docs/interop.md.

## Rules

- Facepunch binaries and content stay in a cache outside the repo (`tools/diff/env.sh`; default
  `$UNDERBOX_CACHE/real-engine2`, override with `REAL_ENGINE2_CACHE`). They are never
  committed, copied into the repo or redistributed. Steam and sbox.game downloads stay off.
- Downloaded files are data. The fetcher only verifies hashes and makes hard links; nothing runs
  from inside the download dir, and the scripts live here.
- Only comparison tooling, docs and divergence summaries in our own words are committed. No dumps or
  disassembly of their binaries.

## The artifacts

sbox-public's `Setup.sh`/`Setup.bat` run `SboxBuild bootstrap`, whose `DownloadPublicArtifacts` step
restores Facepunch's prebuilt files. Its base URL is in that file; `tools/diff/fetch.sh` takes it from
`REAL_ENGINE2_HOST` rather than repeating it here.

- **Versioning:** one JSON manifest per sbox-public commit, `{commit, timestamp, files: [{path,
  sha256, size}]}`. Files are content-addressed blobs keyed by sha256. Not every commit has a
  manifest (4 of the 11 newest at our pin did). Setup walks `git rev-list HEAD` back up to 128
  commits and takes the first hit; `fetch.sh` does the same from `abi/SBOX_PUBLIC_COMMIT`.
- **Our pin:** 1a845a09 has no manifest. The nearest is 6b3aeb2c, two commits earlier. The two
  commits between change only managed code (PlayerController, ModelRenderer, SceneModel animation)
  and animgraph sources; no `.def` and no native source. So the 6b3aeb2c natives match the pin's
  interop table.
- **Contents (6b3aeb2c):** 14,635 files, about 10.3 GB.

| Area | Files | What |
| --- | --- | --- |
| `game/bin/win64` | 90 | engine2.dll and every module, tools (vrad, resourcecompiler.exe, dmx converters), steam_api64, Qt |
| `game/bin/linuxsteamrt64` | 73 | **the full Linux engine**, below |
| `game/bin/osxarm64` | 4 | SkiaSharp, HarfBuzzSharp, steam_api, a Vulkan driver. No engine |
| `game/core` | ~6k | compiled core content: `.shader_c`, `.vmat_c`, `.vtex_c`, models, `.surface_c`, `.sound_c`, prefabs |
| `game/addons`, `game/mount` | ~8k | citizen, menu and base addons; mount plugins (about 6 GB) |
| other | ~300 | EmbeddedRefs, render-test goldens |

Linux natives: `libengine2.so` (exports `igen_engine` and `CreateInterface`), `libtier0`,
`libschemasystem`, `libfilesystem_stdio`, `libmaterialsystem2`, `libmeshsystem`,
`libanimationsystem`, `librendersystemempty`, `librendersystemvulkan`, `libvfx_vulkan`,
`libresourcecompiler` plus a `resourcecompiler` CLI, `libassetsystem`, `libtoolframework2` and
other editor modules, `libsteam_api` and `steamclient.so`, SDL3, slang, dxcompiler, OpenImageDenoise,
openxr_loader, Qt5, SkiaSharp and HarfBuzzSharp. There is no separate soundsystem module.
`libengine2.so` needs glibc 2.38 and finds its siblings through `RUNPATH $ORIGIN`. Every
dependency resolves on the dev box.

`fetch.sh` takes `game/bin/linuxsteamrt64/` and `game/core/` by default: 6,231 files, 2.1 GB of
blobs (aliases such as `libQt5Core.so.5.15.2` are hard links to one blob).

## Running real engine2 here

It runs natively on Linux, so Wine is not needed and there is nothing to install. The dedicated
server boots, loads a local project and plays it.

- **Steam:** not needed. With no Steam client, `SteamGameServer_Init` logs that it falls back to
  unauthenticated mode ("server will not be listed or VAC protected"), and the workshop init fails.
  Nothing else changes and nothing works around it.
- **Game dir:** `UNDERBOX_REAL_ENGINE2=<artifact root>` makes `tools/boot-server.sh` boot in a
  reflinked copy of the usual managed build (`tools/diff/real-game.sh`, under
  `$REAL_ENGINE2_RUNS`) whose `bin/linuxsteamrt64` is the downloaded one, with the compiled
  `game/core` laid over the source core (`REAL_CORE=0` skips that). A separate copy, because
  `install-engine2.sh` keeps a real `libsteam_api.so`, so mixing would leak it into our boots. So
  `match.sh` and `game.sh` run unchanged on either engine.
- **Project resources:** managed registers only `*_c` resources. Real engine2 doesn't compile a
  project's JSON GameResources at mount (the editor does that), so a bare checkout fails with
  "LoadFromFile: Couldn't find scenes/....scene". The native `resourcecompiler` doesn't handle
  managed types. `real-game.sh` therefore runs our `assetc` on the `+game` project first. A
  GameResource `_c` is the source JSON in a resource container, checked against 39 real files
  (docs/real-games.md), so this is a small confound.
- **Noise:** real engine2 logs "Error loading resource file" for editor and debug materials it
  doesn't need on a server. It is harmless.

## Harness

| Piece | What |
| --- | --- |
| `tools/diff/fetch.sh [commit] [prefix...]` | Fetch and verify the artifacts for the nearest manifest |
| `tools/diff/run.sh <probe\|orbmatch\|sweeper\|zoo> [both\|ours\|real] [tol]` | Run a scenario on one or both engines; logs and `report.txt` go to `target/diff/<scenario>/` |
| `tools/diff/compare.py <ours.log> <real.log>` | Rank divergences between two logs' `MATCH:`/`GAME:`/`DIFF:` markers |
| `tools/diff/probe/` | A diff-only project: rigid bodies and traces logged as `DIFF:` markers at ticks 1–150 |
| `tools/diff/run.sh frame <corpus> [both\|ours\|real]` | One client frame per engine (`frame.sh`); PNGs, `diff.png`, `metrics.txt` in `target/diff/frame/<corpus>/`. A game checkout in place of `<corpus>`: docs/differential-render.md "Real games" |
| `tools/diff/real-client.sh <corpus> <out>` | Real's client under Xvfb + lavapipe; grabs the settled window from Xvfb's `-fbdir` framebuffer |
| `tools/diff/real-env.sh` | Sourced by both client boots for real: lavapipe ICD, Steam stand-in and friends shim, web API cut off |
| `tools/diff/lavapipe.sh` | Builds a user-local lavapipe from a pinned Mesa commit into `$UNDERBOX_CACHE/mesa-lvp` (once, about 10 min) |
| `tools/diff/frame.py <ours.ppm> <real.xwd> <out>` | Per-channel mean abs error, % pixels over a threshold, luma means, 8×8-block SSIM |
| `tools/diff/run.sh sound [both\|ours\|real]` | corpus/sound's mixed audio per engine (`sound-run.sh`, `sound.py`); docs/differential-audio.md |

`compare.py` pairs markers by shape (the text with numbers replaced by `#`) and occurrence. **High**
means an event happens on one engine only. **Medium** means paired numbers differ by more than the
tolerance. **Low** means the same events in another order. Same-tick reorderings show up as medium
pairs whose numbers swap.

The probe: a static slab; a sphere and a tilted box dropped from 200; two spheres meeting head-on
at 200 u/s with gravity off; a box sliding at 300 u/s; eight more boxes dropped at other
orientations (`Tilt0`–`Tilt7`); and, at tick 5, seven traces (ray hit, miss and slant, sphere and
box sweeps, a ray onto a moving body, a ray starting inside the slab), `SKIN` sweeps of several
sizes and heights, `SUBSTEP` free falls in a private `PhysicsWorld` stepped with 1–6 sub-steps,
and `ERR*` markers for a static prop given a missing model (its shapes, and rays across its hull).
`ProbeRest.cs` adds `SOLID` sweeps that start inside the slab, `GRAZE` rays along its faces,
`DEPTH` (bodies set down on a slab in a private world) and `SLEEP` (free bodies at a range of
linear and angular speeds, sizes and shapes, gravity off: the step each falls asleep at).
`ProbeJoint.cs` adds hinge motors, springs and friction, joints against their own contacts, and
stack sleep timing; `ProbePush.cs` unjointed overlaps pushed apart and orbmatch's re-aimed push;
`ProbeRefresh.cs` when a persistent contact is refreshed; `ProbeTrike.cs` the zoo trike through its
sign; `ProbeMass.cs` mass queries on bodies of each type (docs/differential-joints.md). Orbmatch's bots log `MATCH:POS` every 5 ticks;
the game watcher logs `GAME:RESTED` for every body at `SETTLED` in scenes of 20 bodies or fewer.

`zoo` needs `SANDBOX=<Facepunch/sandbox checkout>`. Runs take 30 s to 3 min each.

## First divergences (ours vs real engine2 with its compiled core)

Real engine2 is deterministic: four orbmatch runs gave identical markers. Ours now is too (1).
Ranked by severity, with each one's state after the fixes (before → after):

1. **Fixed. Contact and trigger events came in random order.** The orbmatch winner on ours across
   four runs was 1, 1, 0, 1; real gave 0 every time, scoring in creation order (team 0 first)
   whenever both teams score in the same tick. `contacts.rs` iterated a `std` `HashMap`. Now pairs
   and events go in shape creation order (`contacts_tests::same_tick_events_follow_creation_order`).
   After: four runs give winner 0 and byte-identical `MATCH:` markers. Bots' final positions still
   differ from real's, unchanged by the sleep, depth and trace fixes below. The trailing bots'
   push from tick 5 now matches real within 0.01 to tick 70 (persistent contacts, per-step
   restitution, real's 10° refresh: docs/differential-joints.md, 16 and 18); at tick 75 a bot
   scores a tick apart. `Bot0_0` ends at x = −157.5 on ours (before −180, then −138.6), −161.5 on
   real; `Bot1_1` 6.0 apart. Open.
2. **Improved. A tilted box lands differently.** The probe box (pitch 20, yaw 30, roll 10) dropped
   onto the slab. Real settles 5.8 units from the drop point (194.4, 1.5, yaw 28.9) at 0.5 u/s by
   tick 50. Before: ours tumbled onto another face and ended 50 units away (151.2, −11.9, yaw 22.0),
   still at 90 u/s at tick 50. After (with (3); 2 PGS and 3 stabilization iterations per
   integration, `world.rs`): 1.0 units away (199.0, −0.3, yaw 28.3), 0.6 u/s at tick 50. Distance
   from the drop point at tick 150 for the other orientations, real / ours: flat 0.14 / 0.06;
   pitch 45 and roll 45 (balanced on an edge on both engines) 1.3 and 1.1 / 0.3; (30,45,15) 57 /
   53; (60,10,0) 7.9 / 6.7; (10,10,10) 3.3 / 0.3; pitch 89 0.4 / 0.03; (35,45,0) 8.3 / 10.3. The
   tumbling cases are chaotic on both engines; headless (`landing_tests`) the same drops land
   differently again. Resting depth, fixed: real's sink doesn't depend on size or mass. A lone
   body set on the slab in a private world sinks 0.0086 (boxes of 4, 32 and 128, a 1000 kg box, a
   sphere); a three-box stack 0.0257, 0.1399, 0.2057 from the bottom. Before: 0.0060 and 0.0178,
   0.1039, 0.1503, and 15.976 against 15.966 for every resting probe box. After (contact softness
   50 Hz against static bodies, 25 Hz between dynamic ones, `world.rs`): 0.0086 and 0.0254, 0.1392,
   0.2042 (`rest_tests`), and every resting probe box at 15.966. Stacking, landings and the zoo
   tests still pass. Rejected: a 0.4 m speculative margin fitted headless landings better
   but kept props near an animated keyframed body awake forever (the zoo never settled), and
   turning warm-starting off collapsed an 8-box stack; both have tests.
3. **Fixed. Integration substeps.** Real integrates `max(n, 4)` times per `StepSimulation(dt, n)`
   (the private-world free fall is identical for n = 1–4 and fits k = 6 for n = 6); a scene step
   passes n = 2, so k = 4. Ours did n Rapier steps of 4 solver iterations each (k = 8). Now one
   Rapier step with `max(n, 4)` solver iterations. Before: z at tick 25 was 101.570 (real
   101.060). After: 101.060, and every `SUBSTEP` marker matches real to four decimals.
4. **Improved. Sliding friction.** The sliding box stops at x = 65.80 on ours before, 65.43 after,
   65.05 on real (1.1% → 0.6%). On contact, real shows a small vertical pop (vz +1.5, then −3.2)
   that ours doesn't.
5. **Fixed. Core GameResources were absent on ours.** On Sweeper, real registers `ammo=1 decal=47
   prefab=61 scene=1 sound=70 surface=17`; ours registered only `scene=1`, because a real install
   ships core compiled and ours compiled only project resources at mount. Ours now compiles core's
   stale or missing GameResources at `SourceEnginePreInit` (`fs::compile_core`, server and client)
   and registers the same counts; Sweeper shows 0 divergences. The compiled set equals real's
   (196 files, names compared case-blind: `UNDERBOX_REAL_CORE` in `fs/core_tests.rs`). Cost: about
   0.1 s on a fresh game-dir copy (196 compiled), 0.1–0.2 s for the stat walk when up to date.
6. **Fixed. Shape sweeps stopped on the surface.** Real stops every shape sweep 1/32 unit short of
   the surface, measured along the normal: the same for spheres of radius 1, 10 and 50, boxes of
   2, 20 and 100, sweeps from 20 or 300 above, and a 45° slant (which ends 1/32 back along both
   axes). Rays keep no skin. Before: ours ended at z = 10.000 (real 10.031), and a radius-1 sphere
   swept 600 units ended 0.1 inside the slab (parry overshoots long casts). After
   (`trace::SWEEP_SKIN`, plus a recast of the last 4 units): every `SKIN` and `TRACE` marker is
   within 0.004 of real, fraction included.
   Start-solid sweeps, fixed: real reports the normal the probe would have entered through,
   taken 1/32 off the surface. A radius-10 sphere 5 into the slab top moving +x gets
   (−0.8669, 0, 0.4984): the slab's far −x top edge, at distance 10.031 (z = 5/10.031); 1 into
   gives (−0.4416, 0, 0.8972); centre at or below the top, (−1, 0, 0); moving down, (0, 0, 1).
   A zero-length sweep reports the overlap, solid, normal (1, 0, 0). Before: (0, 0, 1) for all,
   and no hit for zero length. After (`trace_rules.rs`: sweep in from outside the shape's bounds,
   then step to the skin): all within 0.002. Left: a start-solid sweep moving out of the slab
   (up, or up and sideways) is not solid on real and hits at fraction 0.8497 with normal
   (0, 0, −1), which doesn't fit any geometry in the scene; ours reports solid at 0.

Agreeing (within 0.15): ray traces, the head-on rebound (±10 u/s after contact, so the same default
restitution), the dropped sphere's resting height, Sweeper's 72 cubes settling and the controller
standing at z = 51 on `Player Cube`, and orbmatch's event set and counts.

Content confound, not engine: without Facepunch's compiled core, real engine2 uses another default
surface (head-on rebound 50 u/s instead of 10, the sphere still bouncing at tick 50) and can't build
Sweeper's models from `.vmdl` source (`error_models=76`). So compare against real **with** the
compiled core, the default.

## Sandbox zoo

Both pass (`GAME:STANDING ... on=Block`, the origin trace hits `Block` at z = 0). Both refuse the
same packages and miss the same package models. Differences:

7. **Fixed. Error models had no collision on ours.** `washing_machine`, `tv` and
   `oil_drum_explosive` get the error model on both engines. On real they rest on the floor
   (`fallen=0`); on ours their ModelCollider had `shapes=0` and they fell forever (`fallen=3`,
   z = −3053). Real's error model has one 14-vertex hull: the outline of the "ERROR" sign, 10 deep
   (x ±5), 100 wide (y ±50), 25.08 tall, its right end bevelled. The probe's `ERRRAY` rays map it;
   `model/physics/error.rs` rebuilds it from those measurements as a 12-vertex hull. After:
   `fallen=0`, and every probe ray across it agrees with real to 0.003. Grazing rays, fixed: real
   misses a ray that only touches a hull's boundary, either running in a face's plane (`down` at
   y = −50) or entering and leaving at one point (`up` at y = ±50, through the bottom face's
   edges). Boxes don't follow that rule: a ray down the slab's side plane hits its top edge on both
   engines. Before: ours hit all three; after (`trace_rules.rs`, hulls only): the `ERRRAY` set is
   identical. Left: the error model's render bounds are our 16-unit stand-in cube; real's are the
   sign's (`ERRMODEL bounds`). A diagonal ray onto the slab's exact top edge reports (1, 0, 0) on
   real and the averaged (0.71, 0, 0.71) on ours.
8. **Fixed. Physics totals.** `GAME:PHYSICS` was `bodies=32 shapes=44` on real and `bodies=31
   shapes=34` on ours; all of the gap was error models (7). After: `bodies=32 shapes=44`.
9. **Fixed. Resource counts** followed (5): ours was `ammo=5 prefab=183 sound=123 surface=7`, now
   `ammo=6 decal=47 prefab=244 scene=6 sound=191 surface=20`, as real.
10. **Improved. Sleeping.** Real keeps the vehicle's bodies (`base`, wheels, `rear`) and
    `hoover_a` awake for longer after the drop: 5 awake for ~15 s on real, while ours has all 16
    asleep within 4 s. Both settle (`GAME:SETTLED`). The probe's `SLEEP` markers give real's rule:
    a body is eligible while `|v| + max |ω × r|` over the corners `r` of its local bounding box
    stays under its threshold (2 u/s by default, `SleepThreshold` honoured), and sleeps after
    0.5 s eligible (step 25 at 50 Hz), velocities zeroed. A radius-8 sphere sleeps at
    ω = 0.16 and not 0.18 (2/(8√2) = 0.177); a 32-unit box at 0.085, not 0.095; an 8×8×64 bar
    at 0.2 about its long axis but not 0.07 about a short one; v = 1 with ω = 0.1 on the
    radius-8 sphere stays awake (1 + 1.13 > 2), so the terms add. A parameterless
    `PhysicsWorld.CreateBody()` has gravity and auto sleep at 2. Before: Rapier's rule
    (`|v| + ω·bounding radius` under twice the threshold) and `CreateBody()` bodies with gravity
    and sleep off, so no private-world body ever slept (30 of the 64 `SLEEP` markers differed).
    After (`sleep.rs` decides eligibility each step, Rapier keeps islands and the timer): all 64
    match. The zoo is unchanged, because the
    gap isn't a threshold: on real the trike's bodies really move for ~15 s, while ours rests.
    Not the hinge motors, which match real exactly: real throws the trike out of the error sign
    it starts inside, and it tumbles 50 units onto its side; ours pushes it 9 units out, upright.
    Unjointed push-out already matches real, and neither its residual speed nor the contact
    fixes move the trike (docs/differential-joints.md, 16, 17 and the zoo hoover). The `TRIKE`
    probe (19) shows ours can't even rest the trike alone: its joints never settle against the
    wheels' contacts inside the base. Open.

## Rendering

Client frames (`run.sh frame`), their metrics, divergences and lanes: docs/differential-render.md.

## Audio

`run.sh sound`: corpus/sound's mixed output from both engines through SDL's `disk` audio driver,
compared per event (docs/differential-audio.md). Runs on both engines once `lavapipe.sh` is built
(below).

## Real's UI on lavapipe

A headless `-nosteam` real client skips its UI and audio ticks: rich presence throws every frame
(docs/differential-audio.md, steps 1–3). With the friends stand-in it got past that, then
segfaulted about 15 s in while drawing the menu UI, before the game loaded.

- **Cause: a Mesa 26.0 llvmpipe bug, not real's.** From a core dump (`ulimit -c unlimited`; the
  core lands in the run copy): the fault is in JIT'd fragment code on a lavapipe worker thread,
  loading `size` from a null `{pointer, size}` SSBO descriptor. The execution mask was all zero,
  yet the code still took the descriptor from the "first active lane", which falls back to lane 0,
  whose pointer was never filled in. Mesa 26.0.8's `ssbo_base_pointer`
  (`gallivm/lp_bld_nir_soa.c`) has no empty-mask guard; Mesa main substitutes a zero-buffer
  descriptor when no lane is active. Real's batched UI shader (`ui_cssbox_batched`, eight
  `StructuredBuffer`s plus bindless textures) is the likely trigger; not confirmed per pipeline.
- **Fix:** `tools/diff/lavapipe.sh` builds lavapipe from a Mesa main commit without root (headers
  from `apt-get download` into a sysroot, LLVM 21 from the system), and `real-env.sh` selects it
  through `VK_ICD_FILENAMES` when present (an explicit `VK_ICD_FILENAMES` wins). Real then loads
  corpus games with the menu, UI and mixer ticking: the hud frame matches ours
  (docs/differential-render.md, 4) and corpus/sound mixes.
- **Tried first, no effect:** `LP_NUM_THREADS=0` (still crashes; one run of five hung) and
  `LP_NATIVE_VECTOR_WIDTH=128` (same crash). Other `LP_*`/`GALLIVM_*` knobs weren't tried once the
  cause was clear. No switch skips the menu: `MenuDll.Initialize` always loads the menu addon (and
  its scene, unless the platform menu runs), and `+game` loads the game after it.
- **Web API:** once the UI ticks, the menu calls sbox.game. `real-env.sh` points real clients'
  HTTP(S) proxy at a closed local port, so those calls fail fast (logged, harmless).
- `real-client.sh` turns the friends stand-in on by default (`UNDERBOX_STANDIN_FRIENDS=` turns it
  off); `sound-run.sh` sets it too.

## Follow-up lanes

- **Joints, stack sleep, contacts, mass queries (11–20):** docs/differential-joints.md.
- **Zoo trike (10):** the trike creeps under its own joints alone, then is pushed out of the sign
  too weakly (`TRIKE`, docs/differential-joints.md, 19; a rigid relax pass doesn't fix it).
- **Orbmatch bots (1):** matches to tick 70; the next gap is a score a tick apart. Real's
  relative-turn extent (`r·√2`, 18) is not reproduced.
- **Error-model render bounds (7):** give the stand-in mesh the sign's bounds (assets lane).
- **Rendering:** docs/differential-render.md.
- **Audio:** real scales `PlayFile`'s delay by pitch, plays 2D sounds about 3.3 dB quieter and pans
  through Steam Audio (docs/differential-audio.md, "Real vs ours").
- **Coverage:** net join and grubs. Add scenarios to `run.sh` and markers to the probe as lanes need them.
