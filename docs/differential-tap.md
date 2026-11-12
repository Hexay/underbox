# Call tap

A `libengine2.so` that records every managed↔native interop call and forwards it to a target
engine2, real's or ours. Two runs of the same scenario then compare call by call, and a divergence
pins to the first slot whose return differs. Rules for real's binaries and logs: docs/differential.md.

## Pieces

| Piece | What |
| --- | --- |
| `crates/tap` | The tap: `libengine2_tap.so`, installed as `libengine2.so` |
| `crates/tap/src/generated/` | One trampoline per slot (`s<i>`) and callback (`c<i>`), and a printer per struct. Written by `tools/abigen` (`tools/regen.sh`) from the same `abi/*.json` as `crates/abi` |
| `tools/diff/tap-install.sh <native-dir>` | Renames the engine to `libengine2_target.so`, points our `lib{vfx_vulkan,filesystem_stdio,steam_api}.so` symlinks at it, and puts the tap in its place |
| `tools/diff/run.sh tap <probe\|corpus\|game> [both\|ours\|real]` | `tools/diff/tap.sh`: one dedicated-server run per engine with the tap, then the comparison |
| `tools/diff/tap.py <ours> <real> [--calibrate <real2>]` | Groups, normalises and ranks the divergences |

`UNDERBOX_TAP_DIR` is the only switch: when it is set, `install-engine2.sh` (ours) and
`real-game.sh` (real) call `tap-install.sh`, so any boot script runs tapped.

## How it hooks in

- **Forwarding every export.** Real's `libengine2.so` exports about 4,500 symbols (statically
  linked libraries among them), data symbols included. Rather than generate stubs for them, the tap
  is linked against an empty `libengine2_target.so` (`build.rs`), so it carries `DT_NEEDED
  libengine2_target.so` with `RUNPATH $ORIGIN`. Managed's `NativeLibrary.GetExport` is a `dlsym`
  on the tap's handle, which searches the tap and then its dependencies: `igen_engine` resolves to
  the tap, every other name to the target. The tap exports only `igen_engine`.
- **The handshake.** The tap's `igen_engine` keeps managed's callback table and hands the target a
  table of callback trampolines, calls the target's `igen_engine` (found with
  `dlopen(RTLD_NOLOAD)` + `dlsym`), keeps the slot pointers it filled in, and overwrites managed's
  table with the slot trampolines. Null target entries stay null. A hash mismatch or no
  `UNDERBOX_TAP_DIR` forwards untapped.
- **Trampolines.** Same ABI signature as `crates/abi`'s thunks. A disabled slot costs one
  `OnceLock` read and one bool before a tail call. `[nogc]` slots are safe: the tap never calls
  managed itself.

## Records

One file per thread, `t<NNN>-<thread name>.tap`, numbered in first-call order. One line per call,
written when it returns (so nested calls come first in the file; sort by `seq`):

```
seq  tick  depth  side  name  args | outs  return
```

`side` is `N` for a slot (managed calls native) and `M` for a callback. Values print so two engines
compare:

| Value | Prints as |
| --- | --- |
| integers, enums, bools, floats | decimal; floats in Rust's shortest round-trip form |
| pointers (`NativeRef`, `void*`, arrays, pointer handles) | `0`, or `p<n>`: first-seen order of the address in this run |
| `[Handle:]` objects | `h<id>` (managed's `HandleIndex`, the same order on both engines) |
| managed objects, string tokens, GUIDs | `m<id>`, `t<hex>`, `g<hex>` |
| strings | decoded, quoted, escaped, cut at 200 characters; the game dir (the server's cwd) as `$GAME` |
| structs (by value or by reference) | `{f1 f2 …}`, every primitive field with nested structs flattened, at most 24 |
| pointer + length buffers | `[len#hash]`: an FNV hash of the first MiB, after the call |
| out/ref arguments | after ` \| `, read after the call |

The tick counts calls to `UNDERBOX_TAP_TICK` (a slot or callback name). It starts at 0 and every
writer is flushed at each tick and at exit.

## Filters

| Variable | Default | |
| --- | --- | --- |
| `UNDERBOX_TAP_DIR` | unset: no tap | where the thread files go; keep it under `$UNDERBOX_CACHE` |
| `UNDERBOX_TAP_SLOTS` | `*` | comma-separated globs on slot and callback names, `!` excludes, the last match wins |
| `UNDERBOX_TAP_CLASSES` | `*` | the same on the native class (slots) or managed class (callbacks): a subsystem |
| `UNDERBOX_TAP_TICK` | `global_SourceEngineFrame` | the call that advances the tick (`tap.sh` uses `IPhysicsWorld_StepSimulation`) |
| `UNDERBOX_TAP_TICKS` | all | `a-b`, `a-` or `a`, inclusive |
| `UNDERBOX_TAP_MAX_MB` | 2048 | total log size; then a `# truncated` line and nothing more |

`tap.sh` maps `SLOTS`, `CLASSES`, `TICK`, `TICKS` and `MAX_MB` (default 1024) onto these.

## Running and comparing

```sh
tools/diff/run.sh tap probe               # ours, real, real2; report in target/diff/tap/probe/report.txt
tools/diff/run.sh tap showroom ours       # a corpus scene on one engine
python3 -I tools/diff/tap.py <ours> <real> --calibrate <real2> --slots 'IPhysics*' --kind return --unique
```

The logs land in `$UNDERBOX_CACHE/underbox-tap/<worktree>/<scenario>/<run>/`: about 25 MB for the
probe and 100 MB for 90 s of showroom per run. The tapped probe server ran to `DIFF:END` in 46 s.

`tap.py` groups calls by (tick, slot) and compares each group as a multiset, so the order of calls
within a tick and across threads doesn't count. Before comparing it masks pointer ordinals and rounds
floats to 4 significant digits (`--digits`, `--keep-ptrs`). With `--calibrate`, a group that differs
between two real runs is noise and is skipped: on the probe that is 3,189 of 40,117 groups (8%), on
showroom 42,516 of 96,439 (44%), because showroom's steps follow the wall clock. Unmatched calls are paired
into `return` (same arguments, different return or out-arguments), `args`, `extra` (ours only) and
`missing` (real only), ordered by tick and then by position within the tick, each with the calls
before it on its thread. `--slots`, `--kind`, `--unique` and `--from-tick` narrow it down.

The default tick is `IPhysicsWorld_StepSimulation`, not the frame: frames follow the wall clock and
load time, but steps line up once the scene runs. Every private `PhysicsWorld` step counts too, so the probe
reaches about 10,200 ticks for its 250 scene ticks. Everything before the first step is tick 0.

## First divergences

Probe and showroom, calibrated against a second real run (2026-11-12). None was in the
differential docs before. "Fixed" means the slot no longer diverges in a re-run probe tap.

1. **Boot (both scenes). Fixed** but for the preloads. `IsRetail` is 1 on ours too (managed then
   skips core shader hotload and queues analytics, which post only when logged in), and
   `GetGameRootFolder` ends in `/`. `Resource.OnResourceLoaded(name, file, size)`: real fires it for
   every compiled resource file it reads, named without `_c`, with the whole file in memory (a
   `.vtex` only its first 128 KiB; the rest streams), on the main thread: inside the getter that
   loads it (`Glue_Resources_GetModel/GetMaterial/GetTexture`, `CreateRawMaterial`), during
   `SourceEnginePreInit`/`SourceEngineInit` for its preloads, and during `SourceEngineFrame` for
   `LoadResourceInManifest` loads and their dependencies. Ours (`crates/engine2/src/resource_loaded.rs`)
   queues every `_c` read through `fs::read` or the shader loader and fires the queue from those
   same slots, never from a `[nogc]` one. Still different: real preloads its error and UI
   resources (`shaders/error.shader`, `materials/error.vmat`, `models/dev/error.vmesh`, dozens of
   shaders) and a manifest's dependencies (`.sound` → `.vsnd`), which ours loads lazily or not at
   all on a server, and sizes differ wherever ours compiled the file itself.
2. **Fixed. Physics world gravity (both).** A new world's `GetGravity` is (0, 0, −850) on real and
   was −800 on ours. The scene sets −850 straight after, but a private `PhysicsWorld` keeps the
   default. Now `physics::DEFAULT_GRAVITY` is −850.
3. **Fixed. Shape local bounds (probe).** `IPhysicsShape.LocalBounds` on a sphere set with
   `UpdateSphereShape(center (1.74, 0, 52.7), r 8)` is (−6.26, −8, 44.7)–(9.74, 8, 60.7) on real,
   and was ±8 around the origin on ours, for two reasons: the slot took the shape's AABB without
   its offset from the body, and `World::update_shape` never moved that offset at all (a built
   collider without a parent keeps its pose in `position`), so an updated sphere also simulated at
   its old centre. Both fixed (`world_tests::updating_a_shape_moves_its_offset`).
4. **Tick 1 (probe).** The sliding box's first step: real gives it vz = +1.5, ours −0.006, and
   its position 0.03 higher. This is the "vertical pop" in docs/differential.md (4). The resting
   bodies' sub-0.01 drifts follow.
5. **GUIDs (showroom). Fixed.** A resource's GUID is its compiled file's `GUID` block (16 bytes,
   .NET `Guid` byte order), which the resource compiler copies from the source's `.meta`
   (`"guid"`); real's `box.vmdl` "GUID" is whatever that meta holds, pointer-like bytes and all.
   `ResolvePathToGuid` and every handle's `GetGuid` read it (`resource_guid.rs`), falling back to
   the `.meta` when ours compiled the file without the block. Raw and procedural resources stay zero
   on both.
6. **Showroom content. Fixed.** On a dedicated server real's `PrecacheSound` returns null for every
   sound, present or not, though it still loads the file; ours returns null there and for a missing
   file anywhere. A new light's radius is 0 (managed sets it for point and spot lights).
   `GetNumMaterialGroups` counts `m_materialGroups`: 0 for the dev models.
7. **Model metadata (probe `MODEL`). Fixed.** A file model without mesh groups has one body part,
   `Body`, whose one choice is the file's stem with mask `u64::MAX`; a procedural one has none.
   `GetSkeletonName` is "" (no core model references a `.vnmskel`). A model's own physics is named
   `embedded_physics.vphys`. Unnamed `CModelBuilder` models are `sbox_procedural_model.vmdl` to
   managed (ours keys their animation data by a unique internal name). Still different: an empty
   `CModelBuilder` model has 0 meshes on real and our stand-in mesh on ours, and models compiled
   from the dev `.vmdl` sources have no physics on ours (`GetPhysicsContainer` null).

## Fixed

From the tap and a scene-state dump, checked with `ProbeBounds.cs` (`BOUNDS`, `STATIC`,
`FRICTION`, `GRAVITY`, `TRI`, `INERTIA` markers; real's values are ground truth):

| What | Real | Ours before | Now |
| --- | --- | --- | --- |
| A body's and a shape's `BuildBounds` | world bounds padded by 0.8 on every axis (`Collider.GetWorldBounds` shrinks them back) | unpadded | padded (`shape_slots::built_bounds`) |
| `BuildBounds` of a body without shapes | inverted (`f32::MAX` … `−f32::MAX`) | zero | inverted |
| `PhysicsShape.Friction` with no collider value | −1 (`PlayerController` resolves it through the trace surface) | the surface's 0.8 | −1 |
| `Sleeping` on a static body | true from creation, before any step | false | true |
| `SceneObject.Bounds` of a model object | world space: the model's render bounds moved by the transform (a rotated, scaled box matches to 4 decimals) | model space (`scene/create.rs` stored them once) | world space (`object::State::world_bounds`), unless managed set bounds |
| `SkinnedModelRenderer.Bounds` (`m_worldBounds`, `m_localBounds`) | the posed bounds | zero (fields never written) | the model's render bounds, moved for world |
| A sphere's triangulation | 402 vertices, 800 triangles | a 62-vertex UV sphere | an octahedron cut 10 times per edge: 402 and 800 (`triangulate::octasphere`) |
| Error model inertia | body-axis order | ascending | body-axis order (docs/differential-joints.md, 21) |
| Which pairs a trigger reports (`ProbeTrigger.cs`, `TRIGGER`: static, keyframed and dynamic triggers over static, keyframed and dynamic boxes) | every pair but static-static, at tick 1. Teleporting a trigger that already overlaps reports nothing new | only pairs with a dynamic body (a522d19: also keyframed-keyframed) | every pair but static-static (`shape::pair_types`) |

The walker check: `MAP=scenes/scenemap.scene SLOTS='IPhysicsBody_*' TICKS=0-12 tools/diff/run.sh tap
<sbox-walker> both`. Before, 447 `IPhysicsBody_Set*` calls diverged from tick 1 (real teleports
bodies ours left alone); after, none. The walker join (`MODE=walker tools/game.sh`) passes with the
map pinned (docs/walker.md); unpinned it fails on the client, as real's managed code would too.

Left: real's posed bounds aren't the render bounds. The citizen at yaw 45 reads local
(−7.0, −29.1, −3.75)–(10.0, 29.1, 71.7) against render bounds (−5.2, −25.7, 0)–(8.6, 25.7, 71.7), and
its world box is tighter than the local box rotated, so real builds it from posed world-space
geometry. Ours loads the citizen as the error model on the server, so the probe can't compare it
yet. Real's sphere vertices sit in another orientation (no vertex at the poles), so only the counts
match.

Noise that calibration doesn't remove: `ConVar.GetName` during registration pairs convars by
position, so different registration orders read as `return`. The per-frame `PerformanceTrace` and
scene-housekeeping calls dominate showroom's `extra` count, because the two engines ran different
numbers of frames per step.
