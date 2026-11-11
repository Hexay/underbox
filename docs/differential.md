# Differential runs against real engine2

The third correctness check in docs/PLAN.md: run the same managed build and the same scripted
scenario on Facepunch's closed engine2 and on ours, and compare what managed code observes. The
user confirmed that the s&box EULA allows this.

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

`compare.py` pairs markers by shape (the text with numbers replaced by `#`) and occurrence. **High**
means an event happens on one engine only. **Medium** means paired numbers differ by more than the
tolerance. **Low** means the same events in another order. Same-tick reorderings show up as medium
pairs whose numbers swap.

The probe: a static slab; a sphere and a tilted box dropped from 200; two spheres meeting head-on
at 200 u/s with gravity off; a box sliding at 300 u/s; and, at tick 5, seven traces (ray hit, miss
and slant, sphere and box sweeps, a ray onto a moving body, a ray starting inside the slab).

`zoo` needs `SANDBOX=<Facepunch/sandbox checkout>`. Runs take 30 s to 3 min each.

## First divergences (ours vs real engine2 with its compiled core)

Real engine2 is deterministic: four orbmatch runs gave identical markers. Ours repeats its positions
exactly but not its event order (1). Ranked by severity:

1. **High. Contact and trigger events come in random order.** The orbmatch winner on ours across
   four runs was 1, 1, 0, 1; real gave 0 every time, scoring in creation order (team 0 first)
   whenever both teams score in the same tick. Cause: `crates/physics/src/contacts.rs` builds
   begin/end events by iterating a `std` `HashMap`, whose seed is random per process.
2. **High. A tilted box lands differently.** The probe box (pitch 20, yaw 30, roll 10) dropped onto
   the slab. Real lands and settles about 6 units from the drop point (194.4, 1.5, 16.0, yaw 28.9);
   at tick 50 it moves at under 1 u/s. Ours tumbles onto another face (pitch −90) and ends about 50
   units from the drop point (151.2, −11.9, 16.0, yaw 22.0); at tick 50 it still moves at 90 u/s. A box-face landing in
   any game goes the same way, so props come to rest in other places.
3. **Medium. Integration substeps.** Velocities match exactly (gravity 850; −17, −68, −153, −408
   at ticks 2, 5, 10, 25), but positions drift: z at tick 25 is 101.570 on ours and 101.060 on real.
   The drop after n ticks fits semi-implicit Euler with k substeps, g·dt²·nk(nk+1)/(2k²), exactly:
   k = 4 for real and k = 8 for ours. The same effect shifts orbmatch's first MOVE positions by
   about 0.4.
4. **Medium. Sliding friction.** The sliding box stops at x = 65.80 on ours and 65.05 on real
   (1.1%). On contact, real shows a small vertical pop (vz +1.5, then −3.2) that ours doesn't. Partly
   the same as (3).
5. **Fixed. Core GameResources were absent on ours.** On Sweeper, real registers `ammo=1 decal=47
   prefab=61 scene=1 sound=70 surface=17`; ours registered only `scene=1`, because a real install
   ships core compiled and ours compiled only project resources at mount. Ours now compiles core's
   stale or missing GameResources at `SourceEnginePreInit` (`fs::compile_core`, server and client)
   and registers the same counts; Sweeper shows 0 divergences. The compiled set equals real's
   (196 files, names compared case-blind: `UNDERBOX_REAL_CORE` in `fs/core_tests.rs`). Cost: about
   0.1 s on a fresh game-dir copy (196 compiled), 0.1–0.2 s for the stat walk when up to date.
6. **Low. Shape sweeps stop on the surface.** Sphere and box sweeps onto the slab end at z = 10.000
   on ours and 10.031 on real: real keeps about 0.03 units of skin. The fraction is the same to
   three decimals. All seven traces agree on hit, start-solid, normal and object.

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

7. **High. Error models have no collision on ours.** `washing_machine`, `tv` and
   `oil_drum_explosive` get the error model on both engines. On real they rest on the floor
   (`fallen=0`, 16 bodies); on ours their ModelCollider has `shapes=0` and they fall forever
   (`fallen=3`, z = −3053 by the end). Real's compiled `models/dev/error.vmdl_c` carries physics;
   ours doesn't. This is the "Falling props" item in docs/real-games.md, now with a reference.
8. **Medium. Physics totals.** `GAME:PHYSICS` is `bodies=32 shapes=44` on real and `bodies=31
   shapes=34` on ours. Three shapes are (7); the other body and seven shapes are not yet attributed.
   Run a per-body shape dump on both before guessing.
9. **Fixed. Resource counts** followed (5): ours was `ammo=5 prefab=183 sound=123 surface=7`, now
   `ammo=6 decal=47 prefab=244 scene=6 sound=191 surface=20`, as real.
10. **Low. Sleeping.** Real keeps the vehicle's bodies (`base`, wheels, `rear`) and `hoover_a`
    awake for longer after the drop (5 or 6 awake against ours' 3, which are the falling props).
    All sleep by the end on real.

## Follow-up lanes

- **Deterministic physics events (1):** emit begin/end events in a stable order. Order by collider
  creation, as real appears to, then add a test that a symmetric scene gives the same order every
  run.
- **Box contact response (2):** compare restitution, friction and angular response for box-on-box
  and box-on-slab landings against the probe. Extend the probe with more drop orientations first.
- **Substep count (3):** find where ours ends up with twice real's integration substeps
  (managed's `subSteps * SubSteps`, Rapier's own solver iterations) and match real's.
- **Error-model collision (7):** give our error model the collision real's has (measure it with
  a probe trace against an error-model prop on real), then recount the zoo's bodies and shapes (8).
- **Trace skin (6):** decide whether to copy real's ~0.03 skin; PlayerController's ground checks
  are the consumer.
- **Coverage:** client-side differentials (frame hashes against `librendersystemvulkan`), net
  join, and grubs. Add scenarios to `run.sh` and markers to the probe as lanes need them.
