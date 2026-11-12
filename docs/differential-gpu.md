# GPU frame differentials against real engine2

Part of docs/differential.md (its rules apply). `run.sh frame` compares pixels, `run.sh tap` interop calls
and `run.sh state` scene objects. `run.sh gpu` compares what each engine submits to Vulkan for one pinned
frame: passes and their attachments, draws, pipelines and shaders, bound textures and samplers, uniform
and push-constant contents, and the effective raster, depth and blend state.

    tools/diff/run.sh gpu <corpus|game-checkout> [both|ours|real]
    DUMP_OURS=19:1 DUMP_REAL=17:0 tools/diff/run.sh gpu showroom   # also copy out two passes' targets

| Output | What |
| --- | --- |
| `target/diff/gpu/<name>/{ours,real}.txt` | Normalized listing per pass and draw |
| `target/diff/gpu/<name>/report.txt` | Where the frames first diverge, divergences grouped by kind, the pass alignment |
| `target/diff/gpu/<name>/dumps/<side>/*.png` | Render-target dumps (`DUMP`) |
| `$UNDERBOX_CACHE/underbox-gpu/<worktree>/<name>/<side>/frame.jsonl` | The raw capture, with uniform contents |

Captures and dumps stay out of the repo (real's are Facepunch output).

## How it works

- **A capture layer of our own**, `tools/diff/gpu/` (C++, about 1.9k lines), built by
  `tools/diff/gpu-layer.sh` into `$UNDERBOX_CACHE/underbox-gpu-layer/<source hash>/` in about 10 s and
  enabled with `VK_ADD_LAYER_PATH` and `VK_INSTANCE_LAYERS`. LunarG's api_dump writes every call as text,
  loses the creation info of objects made before the frame once limited to it, and can't show buffer
  contents or targets. RenderDoc would need its Python replay built with its own swig, and a trigger
  inside ours. The layer writes only the chosen frame, already resolved.
- **Both sides present on Xvfb** through the same local lavapipe (`tools/diff/lavapipe.sh`,
  `VK_ICD_FILENAMES`): ours runs with `SDL_VIDEO_DRIVER=x11` so it has a swap chain too, and
  `WGPU_DEBUG=1` so wgpu labels its passes. Both boot through `tools/boot-client.sh` (seeded content on
  ours, `real-game.sh`, `real-env.sh`, `noweb.sh` on real), Shadow quality High and fog Off as in
  `frame.sh`. Game checkouts get `FramePin.cs` through `tools/diff/pin-project.sh`, shared with `frame.sh`.
- **Frame selection.** A watcher touches `<capture>/trigger` `GRACE` (20) s after `game.load success`.
  The layer arms at the next present, records for `UNDERBOX_GPU_LEAD` (3) frames, then writes the frame
  between the next two presents and touches `done`. The boot stops on the layer's `gpu-capture: done`.
- **What is recorded.** Every `vkCmd*` that matters, per command buffer, with the bound state snapshot
  at each draw. Pipelines are kept from creation, graphics pipeline libraries merged into the linked
  pipeline (real uses them), with dynamic state (real sets cull, depth, bias and stencil dynamically)
  applied over it. Descriptor sets are tracked through writes, copies, templates and push descriptors and
  resolved at submit. After each submit of the frame the layer waits for the queue and reads uniform
  buffers, index buffers and indirect arguments straight from memory: lavapipe's memory is all host
  visible. Arrays over 16 descriptors (bindless) are listed once per frame with their fill.
- **Matching** (`tools/diff/gpu_diff.py`) never uses handles. A draw's mesh is its index count, its
  vertex span and a hash of its indices relative to their minimum: both engines draw real's seeded
  `.vmdl_c`, so the same mesh hashes the same (showroom's box is `b9c596af`, its plane `f624d74f`,
  on both). Passes align in order by role (colour attachment count, depth, render area) plus the set of
  meshes drawn, then by role alone. Within a pass, meshes compare by instance totals (engines batch
  differently), and the first draw of each mesh is paired for state, vertex formats, textures and
  samplers by identity (format, size, mips), and uniforms paired by size and compared per float.
- **Render-target dumps.** `DUMP=<n>[:<draw>]` names pass `P<n>` of that side's listing (submit order)
  and optionally a draw in it. A lead frame learns the pass's attachment signature; later frames copy the
  attachments out at the end of every pass with that signature and drop their draws after `<draw>`;
  the capture keeps the copy at the learned position. Caveat: earlier passes with the same signature are
  truncated too (ours' depth prepass shares its main pass's signature). Formats are converted for viewing:
  float targets clamped and sRGB-encoded, depth stretched over its range.

Limits: `vkCmdSetVertexInputEXT`, maintenance5/6 binding calls and descriptor buffers aren't tracked (neither
engine uses them here). Command buffers recorded more than the lead ahead show as `unrecorded`: real
has two per frame, its first and last submit, still unrecorded with a lead of 3. An indirect count buffer isn't
read; up to the maximum count is listed.

## Findings (showroom and skinned, ours vs real)

Real renderer differences. Pixel metrics already agree closely (docs/differential-render.md), so most
are structure, not output. Fixed items say what changed and what is left.

1. **Reverse Z (fixed).** Every view now clears depth to 0 and tests `GreaterOrEqual`, shadow maps
   included (`SunShadow::reverse_z` is always true). Main views use native's depth range
   [0.02, 1] (`camera::MIN_Z`) and the 3D skybox [0, 0.02], in place of a squeezed projection. The
   1088-byte per-view constants are filled as real's capture lays them out (shading/view.rs,
   shading/view_space.rs): camera-relative `g_matWorldToProjection` with `g_vWorldToCameraOffset`,
   `g_matViewToProjection` stored transposed, FOV in degrees (-1 for ortho), forward-Z multiview
   copies. 7 of 272 floats still differ on the main view: `g_flTime`, `g_vRandomFloats` and
   motion-vector jitter, left at 0 so frames stay deterministic. Shadow views' 1088-byte buffers
   still differ (item 2).
2. **Shadow maps.** Real draws three 2048² sun cascades, back faces culled, with dynamic depth bias
   (constant −1, slope −1.5), each caster once with three instances; ours draws two cleared plus six
   loaded 2048² passes (the six are likely the lamp's cube faces in one atlas) with culling off, no
   bias, one draw per caster. Real renders local-light shadows into seven separate 512² passes.
3. **Depth format and stencil.** Real's main depth is `D32SfloatS8Uint` with stencil test on in the
   opaque pass; ours is `D32Sfloat`, stencil off.
4. **Depth prepass.** Real's is depth only, three passes of indirect draws after its GPU occlusion cull
   (some with zero instances); ours writes a colour target too, direct draws.
5. **Vertex streams.** Real's static meshes take position, `R16G16Sfloat` UV, `A2B10G10R10` packed
   normal and tangent, plus per-instance `R32G32B32A32Sfloat`/`R32Uint`; ours feeds unpacked 32-bit
   floats. Skinned meshes differ more (real: `R32Sfloat` weights, `R8G8B8A8Uint` indices).
6. **Constants ours left zero (fixed).** The 592-byte buffer now matches byte for byte: native's
   default wind (also what `GetSceneWorldWind` returns), the camera as
   `g_vHighPrecisionLightingOffsetWs`, and infinite fog culling bounds. Real's 80-byte cluster block
   comes from managed's `ClusteredCullingLayer` on the view's attributes; layers' attributes now
   inherit the view's, so a shader that declares it gets managed's grid, identical to real's. The
   80-byte buffer still paired against it on the main draw is the material stand-in's own globals
   (the stand-in reads no clusters). The 304-byte buffer is byte-identical. The 4128-byte light array
   holds NaN in real's unused slots, 0 on ours.
7. **Morph targets.** Real composites morphs in a 2048×1024 `R16G16B16A16Sfloat` pass of two additive
   draws before shadows; ours has no such pass (compute).
8. **Final output (mostly fixed).** A pass that neither clears nor draws isn't recorded (showroom:
   33 passes to 16, real 21). `TargetSupportsSrgbWrite` is true for 8-bit targets, so managed's UI no
   longer takes the float path (the "UI Gamma Seed" / "Composite" blits through `R16G16B16A16Sfloat`);
   the last pass writes the swap chain image as real's does. Left: the swap chain image is ours'
   own texture, copied to the surface at present (engine textures can't wrap a surface frame), and
   387 (skinned: 638) transfers against real's 16, mostly small buffer copies and fills.
9. **Bindless.** Real's array holds 1252 views (every loaded texture); ours 5 per set, with material
   textures bound directly. Per-draw texture lists therefore can't be compared on real's side yet: its
   material textures are indexed from uniforms.

Next steps: decode real's bindless indices from its per-draw constants so material textures compare per
draw; name real's shaders (their SPIR-V carries no source string) by matching stage interfaces.
