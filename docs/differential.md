# Differential runs against real engine2

The third correctness check in docs/PLAN.md: run the same managed build and the same scripted
scenario on Facepunch's closed engine2 and on ours, and compare what managed code observes. The
user confirmed that the s&box EULA allows this.

## Rules

- Facepunch binaries and content stay in a cache outside the repo (`tools/diff/env.sh`; default
  `$XDG_CACHE_HOME/underbox/real-engine2`, override with `REAL_ENGINE2_CACHE`). They are never
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

`zoo` needs `SANDBOX=<Facepunch/sandbox checkout>`. Runs take 30 s to 3 min each.

## First divergences (ours vs real engine2 with its compiled core)

Real engine2 is deterministic: four orbmatch runs gave identical markers. Ours now is too (1).
Ranked by severity, with each one's state after the fixes (before → after):

1. **Fixed. Contact and trigger events came in random order.** The orbmatch winner on ours across
   four runs was 1, 1, 0, 1; real gave 0 every time, scoring in creation order (team 0 first)
   whenever both teams score in the same tick. `contacts.rs` iterated a `std` `HashMap`. Now pairs
   and events go in shape creation order (`contacts_tests::same_tick_events_follow_creation_order`).
   After: four runs give winner 0 and byte-identical `MATCH:` markers. Bots' final positions still
   differ from real's by up to 18.5 units (physics, not order).
2. **Improved. A tilted box lands differently.** The probe box (pitch 20, yaw 30, roll 10) dropped
   onto the slab. Real settles 5.8 units from the drop point (194.4, 1.5, yaw 28.9) at 0.5 u/s by
   tick 50. Before: ours tumbled onto another face and ended 50 units away (151.2, −11.9, yaw 22.0),
   still at 90 u/s at tick 50. After (with (3); 2 PGS and 3 stabilization iterations per
   integration, `world.rs`): 1.0 units away (199.0, −0.3, yaw 28.3), 0.6 u/s at tick 50. Distance
   from the drop point at tick 150 for the other orientations, real / ours: flat 0.14 / 0.06;
   pitch 45 and roll 45 (balanced on an edge on both engines) 1.3 and 1.1 / 0.3; (30,45,15) 57 /
   53; (60,10,0) 7.9 / 6.7; (10,10,10) 3.3 / 0.3; pitch 89 0.4 / 0.03; (35,45,0) 8.3 / 10.3. The
   tumbling cases are chaotic on both engines; headless (`landing_tests`) the same drops land
   differently again. Left: resting boxes sit 0.010 higher on ours (15.976 against 15.966: real
   sinks 1/32 into the slab). Rejected: a 0.4 m speculative margin fitted headless landings better
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
5. **Medium. Core GameResources are absent on ours.** On Sweeper, real registers `ammo=1 decal=47
   prefab=61 scene=1 sound=70 surface=17` from core; ours registers `scene=1`. Ours compiles only
   project resources at mount, never core's JSON sources. Games that look up core surfaces,
   sounds or prefabs by path get nothing. Real given a source-only core also registers `scene=1`,
   so the engines behave alike on equal content; the gap is that a real install ships the compiled
   core.
6. **Fixed. Shape sweeps stopped on the surface.** Real stops every shape sweep 1/32 unit short of
   the surface, measured along the normal: the same for spheres of radius 1, 10 and 50, boxes of
   2, 20 and 100, sweeps from 20 or 300 above, and a 45° slant (which ends 1/32 back along both
   axes). Rays keep no skin. Before: ours ended at z = 10.000 (real 10.031), and a radius-1 sphere
   swept 600 units ended 0.1 inside the slab (parry overshoots long casts). After
   (`trace::SWEEP_SKIN`, plus a recast of the last 4 units): every `SKIN` and `TRACE` marker is
   within 0.004 of real, fraction included. Left: a sweep that starts inside the slab reports
   normal (0, 0, 1) on ours and a slanted one on real.

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
   `fallen=0`, and every probe ray across it agrees with real to 0.003, except that ours also hits
   rays grazing the exact y = ±50 ends. Left: the error model's render bounds are our 16-unit
   stand-in cube; real's are the sign's (`ERRMODEL bounds`).
8. **Fixed. Physics totals.** `GAME:PHYSICS` was `bodies=32 shapes=44` on real and `bodies=31
   shapes=34` on ours; all of the gap was error models (7). After: `bodies=32 shapes=44`.
9. **Medium. Resource counts** follow (5): real `ammo=6 decal=47 prefab=244 sound=191 surface=20`,
   ours `ammo=5 prefab=183 sound=123 surface=7`; the excess is core's compiled resources.
10. **Low. Sleeping.** Real keeps the vehicle's bodies (`base`, wheels, `rear`) and `hoover_a`
    awake for longer after the drop: 5 awake for ~15 s on real, while ours has all 16 asleep
    within 4 s. Both settle (`GAME:SETTLED`).

## Follow-up lanes

- **Contact depth (2):** real rests boxes and props 1/32 into the surface; ours 0.024. Try
  matching Rapier's allowed linear error to it, then rerun the probe.
- **Core GameResources (5):** compile `game/core`'s JSON resources at mount, or with `assetc` in
  `game-dir.sh`, then compare Sweeper's resource counts again.
- **Error-model render bounds (7):** give the stand-in mesh the sign's bounds (assets lane).
- **Coverage:** client-side differentials (frame hashes against `librendersystemvulkan`), net
  join, and grubs. Add scenarios to `run.sh` and markers to the probe as lanes need them.
