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
- **wgpu indirect validation is off.** It added a dispatch and a copy per indirect pass, and
  native validates none. `WGPU_VALIDATION_INDIRECT_CALL=1` restores it.

## Before / after

Merged base against this change: same build flags, 3 interleaved rounds, ours only.

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
  particles and decals equal docs/differential-render.md's latest column. `run.sh gpu` loses
  wgpu's validation dispatches (showroom 25 → 17 dispatches, skinned 27 → 20), and the rest is
  unchanged. Its "bindless textures" count for ours drops, because reused groups write no
  descriptors in the captured frame.

## Left, and negative results

- **Skinned raster is still 10% over real.** Ours draws from the compute-skinned vertex cache;
  real skins from the mesh's float stream (docs/differential-gpu.md item 5).
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
