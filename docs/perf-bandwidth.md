# DRAM read traffic: where it comes from

Method and totals: docs/differential-perf.md ("DRAM reads", "Where DRAM reads come from",
"Memory bandwidth, 2026-11-19"). Rankings below are `tools/diff/perf_attrib.py` over that sweep's
5 rounds per engine, medians, MB per frame (clients) or per tick (servers), ours / real. Real is
shown by DSO only (its symbols are its own binaries'); lavapipe and libc are the same on both.

## Clients (showroom | skinned)

| Hot spot | Thread | Showroom | Skinned | Whose |
| --- | --- | --- | --- | --- |
| `lp_rast_clear_zstencil`: three 2048² `D32Sfloat` shadow cascades cleared per frame (16 MiB each) | lavapipe | 54.3 / 55.7 | 51.8 / 52.3 | managed's passes; same on both (`run.sh gpu`) |
| Compute shaders (JIT code: ours unsymbolised, real's perf-mapped) | unnamed | 33.6 / 49.3 | 34.7 / 50.5 | ours lower |
| Fragment shaders (JIT code) | lavapipe | 45.4 / 49.9 | 33.4 / 34.3 | |
| `util_fill_rect`: colour clears | lavapipe | 6.0 / 8.9 | 21.9 / 24.6 | managed's passes |
| `memmove` in lavapipe's queue thread: buffer and image copies | unnamed | 16.3 / 13.7 | 16.4 / 13.6 | **ours +2.6**: CMAA2 texel words (below) |
| Binning and tile walk (`lp_scene_bin_iter_next`, `rasterize_scene`, `lp_rast_shade_tile`) | lavapipe | 9.6 / 9.6 | 9.4 / 9.3 | |
| .NET JIT (`libclrjit`) in the window | engine | 3.7 / 0.7 | 3.4 / 1.1 | runtime tiering, not engine2 |
| Main thread, all DSOs | main | 9.1 / 10.5 | 11.1 / 9.8 | parity; ours flat (largest: `memmove` 0.4–0.7, `_mi_page_malloc_zero` 0.2–0.3) |

Thread classes in total: lavapipe 119 / 132–135, unnamed 57–58 / 73–74, engine 7–8 / 12–13.
Ours is lower than real in every class but the queue thread's copies.

## Servers

| Hot spot | Pile | Probe |
| --- | --- | --- |
| Main thread, total | 5.95 / 3.98 | 2.28 / 2.81 |
| engine2 code (main + other threads) | 3.18 / 1.41 | 0.64 / 0.51 |
| Managed code (`memfd:doublemapper`) | 1.82 / 1.90 | 0.87 / 0.94 |
| Other threads (.NET JIT, real's job pool) | 1.28 / 2.36 | 0.60 / 0.96 |

Pile's excess is Rapier's per-tick working set, spread thin: `World::step` 0.47 (our per-step walks
inlined into it), contact constraints 0.36, the island solver 0.14, narrow phase 0.12, then interop
body slots and `body_cache::resolve` ~0.1 each. 328 bodies need ~50k lines per tick: on a shared
32 MiB L3 the state is evicted between 20 ms ticks and refetched every tick, so the count tracks
the bytes a step touches, not how many times.

## Not changed, and why

- **CMAA2 texel words: the one ours-only hot spot.** `shading/texel_words.rs` copies the
  512×384 `R32Uint` heads image into its word buffer before the atomic pass and back after (two
  768 KiB copies a frame), because CMAA2's clear and walk passes reach the image directly.
  Confirmed by `UNDERBOX_SKIP_LAYERS=cmaa2_cs` (3 rounds, skinned): the queue thread's `memmove` fell
  16.4 → 14.0, real's 13.6. Removing the copies needs the words to be the texels' only home: every
  module touching the image rewritten to words, and clears, copies and samples of the texture
  synced. That is a SPIR-V pass change (PASSES_VERSION) with golden-frame risk for 1.2% of a
  frame's traffic. Left open.
- **Pile's Rapier working set** (+1.8 MB/tick, 0.09 GB/s): no single owner; shrinking it means
  Rapier's layout or our side tables (`step_start`, `frozen`, `sleep_thresholds`, about 0.1 MB in all),
  under the byte-identical `run.sh state` lock. Not attempted.
- **Shadow clears** are the biggest single item, but managed asks for them and real does the same.
- **Main-thread double copy in `render/batch.rs`** (`staged` Vec, then wgpu's staging buffer):
  0.4–0.7 MB/frame, below the main thread's ±2 MB run-to-run spread, so an A/B can't resolve it.
- **IBS (`perf mem`)** works per process at paranoid 1 and gives data addresses, but it samples demand
  loads only: a streaming loop's prefetched lines read as L1/L2 hits. Fill-event sampling was used.
- **Coverage with `RECORD` on:** "dram per-thread coverage" read up to 1.12–1.18 in single server
  runs on both engines (medians 1.00; the one pile run without `RECORD` read 1.00). Unexplained;
  take totals from runs whose coverage is 1.00, or without `RECORD`. Later seen once without
  `RECORD` too (1.15, pile, 2026-11-19).

## Pile: physics working set, 2026-11-19

Tools: `RECORD` server runs, symbols of ours; A/B is `REPEATS=1 UNDERBOX_ENGINE2=<so> run.sh perf
pile ours`, builds alternating. Headless:
`PILE_DRAM=256 cargo run --release -p physics --example pile` reads 256 MiB before each step, then
counts that step's L2 fills from DRAM on its thread: the bytes a step touches with nothing cached
(±0.05 MB run to run, unaffected by the box's load). All runs IL managed (`tools/r2r-pending.py`
listed 79 IL-only dlls in the game copy).

Ranking, base build, 3 `RECORD` runs, MB/tick on the main thread unless noted (total 6.8–7.4 at
load 25–35; real has no symbols here):

| Item | MB/tick |
| --- | --- |
| Managed JIT code / CLR / JIT thread (same work as real) | 1.75–1.83 / 0.47 / 0.50–0.67 |
| Rapier contact constraint `generate` (gathers each manifold's 100-byte `TrackedContact`s) | 0.39–0.54 |
| `World::step`: our passes inlined (frozen-pair map lookups, `arm_sleep`'s first touch of every body) | 0.47–0.49 |
| Kernel on main (pacer sleeps, scheduler; `MIMALLOC_PURGE_DELAY=-1` moved it −0.03) | 0.23–0.41 |
| Getters' `body_cache::resolve` (map bucket, then the snapshot line; ×3 instances) | 0.28–0.30 |
| Rapier `run_worker`, `process_pair`, `ColliderSet::get_mut_internal` | 0.14, 0.12, 0.10 |
| `corner_speed`, `has_any_active_contact` (our passes) | 0.08–0.10 each |
| `events::simulate` publish + `Vec<Active>` | 0.10 |
| Resource sweep each engine frame (`resource::unload_unreferenced`, every cache's map) | 0.04 |

Headless per pass (temporary probes between `World::step`'s passes, not kept), MB/step:
`arm_sleep` 0.21, `record_step_start` 0.04, `force_turned_contacts` 0.29, Rapier 1.6–1.75,
`release_forced_contacts` 0.07–0.12, bounce 0.07–0.12, `Contacts::update` 0.18 (0.12 in `touches`),
`collect_activity` 0.04. Rapier alone (every wrapper pass skipped) touched 2.0–2.1 MB a step
against 2.6–2.7 with them. Reading every pair, manifold, point and collider again right after
Rapier's step cost nothing headless: Rapier touches all of them, so the wrapper's own share is
its tables and the shapes `touches` and `corner_speed` read.

Changed (behaviour-identical: example hashes for keyframed and static walls, `run.sh state` on the
pile to tick 750 byte-identical, `tools/test.sh physics` and `engine2 physics` pass):

- **Snapshots in a dense per-world table** (`engine2/src/physics/body_snapshot.rs`), indexed by the
  body's arena slot and tagged with its generation, instead of a 64-byte-aligned field in each
  `Body` (256-byte stride). Managed's passes run in creation order, so they read consecutive lines.
- **One post-step pass** (`physics/src/settle.rs`): frozen-pair state, bounce and the touch list
  per active pair, instead of three passes. In the server the pairs were refetched between them.
- **Frozen pairs updated in place** (`persist.rs`), not rebuilt into a second map each step;
  `Touch` is 40 bytes (its handles are in its ranks); step-start rotations 24 bytes a slot, not
  48; `take_activity` leaves a sized `Vec` instead of regrowing from empty every step.

Results, 4 interleaved rounds (load 25–35): DRAM MB/tick base 7.08–8.28 (median 7.45), new
7.27–7.44 (median 7.33); tick CPU median 2.87–3.46 → 2.82–3.29 ms. By symbol (3 `RECORD` rounds
each): `World::step` 0.49 → 0.36, getter closures 0.07 → 0.04, the new `bounce_pair`, `keep`,
`finish` and table `publish` +0.11: about −0.1 MB/tick, inside the totals' ±0.5 spread. Headless:
DRAM per step unchanged (2.45 vs 2.49 median), instructions 4.71 → 4.69 G keyframed, 4.15 → 4.13 G
static. The 0.9 MB gap to real (6.42) is not closed; at this load base itself read 7.1–8.3.

Tried, not kept:

- **Bounce without `get_pair_mut` when nothing bounces** (read-only check first): `get_pair_mut`
  queues both bodies as user-modified on every manifold every step, but skipping it changed the
  static-wall hashes (the modified queue feeds Rapier's sleep handling). Keyframed hashes held.
- **`MIMALLOC_PURGE_DELAY=-1`**: kernel fills −0.03 MB/tick; the kernel's share isn't page faults.
- **Body type in the snapshot**: managed caches `PhysicsBody3d.BodyType`, so `GetType` isn't per tick.

Left open: Rapier's points gather in `generate` (parry's `TrackedContact` layout); the getters'
map bucket + snapshot (two lines per call; one line needs the snapshot inside the thread's cache
entry, 64 bytes with its key, which leaves no room for the world pointer); the per-frame resource
sweep (a resource-cache throttle changes when unloaded paths reload).
