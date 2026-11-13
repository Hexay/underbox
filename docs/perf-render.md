# Client frame profile: showroom and skinned

Linked from docs/differential-perf.md (how the numbers are taken). Lavapipe, `taskset 0-7`,
`LP_NUM_THREADS=4`, 200-frame window, CPU ms per frame, medians. The machine is shared: compare
only runs interleaved with each other.

## Where a frame's CPU goes (before)

Showroom on the merged base; skinned in brackets. Real's from `run.sh perf <scene> both`.

| # | Part | Ours | Real |
| --- | --- | --- | --- |
| 1 | Lavapipe raster threads (`llvmpipe-*`) | 140 [108] | 129 [92.7] |
| 2 | Lavapipe compute pool (unnamed, counted as engine) | 75.6 [74.5] | 88 [88.1] |
| 3 | Main thread (frame build, wgpu, driver calls) | 7.3 [8.8] | 4.8 [4.85] |
| 4 | .NET tiered JIT thread | 2–5, varies per run | ~3 |

- **Raster.** The stand-in's main draw dominates. It computed the bent normal for every pixel
  even with `g_flStandinBentNormals` off, and bound full 2048-entry bindless sampler arrays,
  where real binds 15.
- **Compute pool.** Managed's compute passes (cluster culling, occlusion cull, SPD, contact
  shadows, CMAA2), plus skinning on skinned. Ours already costs less than real's, though our SPIR-V
  is 3–8× larger (screen_space_shadows 89 KB against 11 KB).
- **Main thread**, by `perf record --call-graph dwarf -t <main tid>`, sliced to the window:
  - About 25% is lavapipe creating and freeing a descriptor set per bind group. Each set maps
    its own memfd memory (mmap, fallocate, munmap), and every draw rebuilt its groups each frame.
  - About 10% is SipHash on the render-attribute map (`(Kind, token)` keys) and the objects
    registry (address keys).
  - About 10% is malloc and free.
  - 2–3% is `Space::of`, a matrix inverse that ran once per member of the view block.

## What changed

- **Bind groups are reused across frames** (`render/src/bind_cache.rs`). Each group is keyed by
  its layout and its resources' identities. Upload slices land at the same offsets while the
  draw order holds. A group unused for a frame is dropped. A failed `Gpu::checked` scope clears
  the cache.
- **Faster hashing.** The attribute map uses FxHash. `StringToken::of` no longer allocates. The
  sampler cache keys a fixed byte array. The objects registry got FxHash from the physics lane.
- **The view block** takes its `Space` apart once (`View::constants`).
- **The stand-in** computes the bent normal only under its uniform flag.
- **Bindless sampler arrays** are partially bound, one past the registered states.
- **wgpu indirect validation stays on in the engine**, because managed (untrusted game code)
  fills the indirect args and the check clamps bad ones. It costs a dispatch and a copy per
  indirect pass, about 2.4 ms of engine CPU per showroom frame. `perf.sh` and `gpu.sh` default
  `WGPU_VALIDATION_INDIRECT_CALL=0` to compare with real, which validates none.

## Before / after

Merged base against this change: same build flags, 3 interleaved rounds, ours only. The base
had indirect validation on; the after runs had it off (`WGPU_VALIDATION_INDIRECT_CALL=0`, now
`perf.sh`'s default). That accounts for about 2.4 ms of the engine CPU drop on showroom and none
of the main-thread or lavapipe drop.

| Metric | showroom before | showroom after | skinned before | skinned after |
| --- | --- | --- | --- | --- |
| Engine CPU | 87.5 | 85.8 | 88.3 | 86.5 |
| Lavapipe CPU | 140 | 133 | 108 | 102 |
| Main thread | 7.3 | 5.1 | 8.8 | 6.4 |
| Unnamed (compute pool) | 75.6 | 75.1 | 74.5 | 74.6 |
| Frame wall | 169 | 70.5 | 80.9 | 67.3 |
| Peak RSS MiB | 1243 | 1233 | 1251 | 1236 |

- Against real: showroom engine 0.83, lavapipe 1.03; skinned engine 0.84, lavapipe 1.10.
- Frame wall is load-dependent. Showroom's drop tracks the munmap traffic that is now gone,
  since munmap stalls every lavapipe thread's page faults.
- Peak RSS is a third of real's, so no heap work was done.
- Rendering is unchanged: `run.sh frame` metrics on showroom, outdoor, fog, skinned, glass,
  particles and decals equal docs/differential-render.md's latest column. With
  `gpu.sh`'s `WGPU_VALIDATION_INDIRECT_CALL=0`, `run.sh gpu` loses wgpu's validation dispatches
  (showroom 25 → 17 dispatches, skinned 27 → 20), and the rest is unchanged. Its "bindless textures" count for ours drops, because reused groups write no
  descriptors in the captured frame.

## Skinning and morphs as real's

The GPU morph composite, real's vertex-cache streams and `-fspv-preserve-interface`
(docs/skinning.md, docs/differential-gpu.md item 7). Medians of 5 interleaved runs per binary, ms:

| Metric | showroom before | showroom after | skinned before | skinned after |
| --- | --- | --- | --- | --- |
| Engine CPU | 86.5 | 85.0 | 86.3 | 86.3 |
| Lavapipe CPU | 131 | 133 | 99.5 | 102 |

- Real, 3 runs: showroom engine 84.1, lavapipe 120; skinned engine 99.1, lavapipe 91.7.
- Lavapipe's ~2 ms rise is on both scenes, so likely the unread inputs now fetched (real fetches
  them too) rather than the composite pass; not A/B'd alone. Frame wall moved with load (20–30) and isn't compared.
- Frame metrics on the seven scenes equal docs/differential-render.md's latest column.

## Skinned raster against real: samplers and the blit

Lavapipe's raster time per JIT function came from a Mesa `PROFILE` build: its perf map, with
`MESA_SHADER_CACHE_DISABLE=true` so cached shaders get named. On both sides the main stand-in
fragment shader and the shadow-compare sampling cost the same (~29 and ~18 ms). The excess was
in four places:

| Hot spot | Ours | Real | Fix |
| --- | --- | --- | --- |
| Stand-in AO sampled anisotropically | 5.9 | 2.1 | `g_sTrilinearWrap`, as `vr_shared_standard_ps_code` |
| CMAA composite, linear/repeat | 2.6 | 0.45 | Vfx `Filter`/`Address*` annotations honoured (`shading/sampler_state.rs`) |
| `MSAAUtils::SampleRed`'s `g_sPointClamp`, linear | 1.8 | 0.34 | same |
| Final blit: `pow` encode, uniform branches, `textureLoad` | ~6.4 | ~2.5 | sRGB target view, one entry point per transfer, nearest sample |

Previously every Vfx sampler was linear/repeat apart from anisotropy and compare. Clamping
changes fog's screen-edge columns by up to 15, now nearer real's; every other frame moves by ≤1 LSB
(the blit's sRGB rounding). Medians of interleaved `run.sh perf skinned` runs, ms:

| Metric | Before (3 runs) | After (5 runs, noisy load) | Real before / after |
| --- | --- | --- | --- |
| Lavapipe CPU | 103 | 93.4 | 91.8 / 88.3 |
| Ours / real | 1.12 | 1.06 | |
| Engine CPU | 85.2 | 80.2 | 94.6 / 90.0 |

- Against real's 91.7 from "Skinning and morphs as real's", 93.4 is 1.02. Real's own median
  moves by ±3 between sessions.
- Frame against real: skinned 0.06 / 0.18 / 0.33 → 0.03 / 0.17 / 0.34 (SSIM 0.9998), and fog
  0.13 / 0.16 / 0.21, 0.9992 → 0.11 / 0.09 / 0.25, 0.9993.

## Left, and negative results

- **Skinned raster is within noise of real.** About 1 ms is left in the blit's nearest sample
  (`r16g16b16a16_float` key 20: 1.7 ms against real's ~1.0).
- **Main thread is still above real's**, more so on skinned. What's left is attribute lookups
  by name per draw (`uniform_value`, `names`), malloc in bind-group building, and wgpu's pass
  encoding.
- **Removing wgpu's shader runtime checks gained about 2% of lavapipe**, so it was reverted.
  Bounds, binding-array and image checks are already left to driver robustness on Vulkan; what
  remained (index clamping, loop bounding, integer division) is cheap.
- **`perf stat --per-thread` attached to the client read about 10× low** against schedstat, so
  instruction counts per frame aren't reported.
- **Lavapipe's JIT code can't be symbolised** without a Mesa `PROFILE` build (its perf map).
  `UNDERBOX_SKIP_LAYERS` and `run.sh gpu` attribute raster cost instead.
- **`perf record -t` needs `-m 128`.** `perf_event_mlock_kb` is 516.
