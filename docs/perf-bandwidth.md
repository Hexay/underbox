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
  take totals from runs whose coverage is 1.00, or without `RECORD`.
