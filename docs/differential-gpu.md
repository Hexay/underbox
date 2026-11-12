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

- **A capture layer of our own**, `tools/diff/gpu/` (C++, about 2.3k lines), built by
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
  visible. Arrays over 16 descriptors (bindless) are listed once per frame with every filled slot.
  Storage buffers are read too (up to 512 KiB), and instance-rate vertex streams.
- **Textures.** The layer hashes every image upload (`gpu/uploads.cpp`): per mip, the sum of CRC-32s
  of its block rows, each seeded by (layer, z, row, x), so neither region split nor row pitch matters.
  Ours labels textures with their resource path (a generated one by its source: `x_tga_<hash>.generated.vtex`
  is `x.tga`); real's are named by hashing mip 0 of every seeded core
  `.vtex_c` of the same size the same way (`gpu_vtex.py`; real uploads block-compressed data as
  stored). Flat defaults share content across many files and list as `name (+N same)`. A draw's
  textures (`gpu_tex.py`) are those bound directly plus the bindless slots its constants select:
  `FIELDS` lists those fields, found with `gpu.py fields <frame.jsonl> [core]` (fields whose values
  are always filled slots) and checked against the public structs. Real: `DirectionalLightCB` +288
  (cascades) and +312 (contact shadows), `ProjectedShadows[]` +64, `ProjectedCubeShadows[]` +396,
  `GPUDecal` words 10 and 11 (low 16 bits); ours: the same contact-shadow field and its decal
  buffer's same words (its binding moves when the stand-in's resources change). Draws then compare
  texture sets by name (else format, size, mips), and same-named textures by format.
- **Matching** (`tools/diff/gpu_diff.py`) never uses handles. A draw's mesh is its index count, its
  vertex span and a hash of its indices relative to their minimum: both engines draw real's seeded
  `.vmdl_c`, so the same mesh hashes the same (showroom's box is `b9c596af`, its plane `f624d74f`,
  on both). Passes align in order by role (colour attachment count, depth, render area) plus the set of
  meshes drawn, then by role alone. Within a pass, meshes compare by instance totals (engines batch
  differently), and the first draw of each mesh is paired for state, vertex formats, textures (above)
  and samplers, and uniforms paired by size and compared per float.
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

## Findings (showroom, skinned and decals, ours vs real)

Real renderer differences. Pixel metrics already agree closely (docs/differential-render.md), so most
are structure, not output. Fixed items say what changed and what is left.

1. **Reverse Z (fixed).** Every view now clears depth to 0 and tests `GreaterOrEqual`, one
   convention for views and shadow maps (`render/depth.rs`). Main views use native's depth range
   [0.02, 1] (`camera::MIN_Z`) and the 3D skybox [0, 0.02], in place of a squeezed projection. The
   1088-byte per-view constants are filled as real's capture lays them out (shading/view.rs,
   shading/view_space.rs): camera-relative `g_matWorldToProjection` with `g_vWorldToCameraOffset`,
   `g_matViewToProjection` stored transposed, FOV in degrees (-1 for ortho), forward-Z multiview
   copies. 7 of 272 floats still differ on the main view: `g_flTime`, `g_vRandomFloats` and
   motion-vector jitter, left at 0 so frames stay deterministic. Shadow views' 1088-byte buffers
   still differ (item 2).
2. **Shadow maps (fixed).** Real lets managed's `ShadowMapper` plan every shadow view
   (callbacks 38–42, then `g_pSceneSystem_AddShadowView`); ours now does the same
   (docs/lighting.md), so the passes match: three 2048² D32 cascades and 512² D16 local maps
   (showroom: six lamp cube faces, one spot), clear 0, `GreaterOrEqual`, back faces culled, bias
   constant −1 / slope −1.5, each caster mesh one instanced draw. Cascade 0's depth dump matches
   real's, and `DirectionalLightCB` differs only in bindless indices and a tiny sun direction
   difference. Casters draw through a depth-only stand-in (`shading/standin_depth.shader`) with
   real's inputs: the shadow view's 1088-byte constants, `g_TransformBuffer` and real's vertex
   formats (position, instance id, blend indices); skinned casters read the vertex cache. Order in
   every opaque pass is real's (`draw::sort_opaque`): rigid before vertex-cached, then larger index
   count first, stable otherwise. It isn't distance: on fog the ground is nearest yet real draws the
   box first. The cascade fit matches: real takes the sun's direction from its unnormalized
   rotation (+X rotated, then normalized; `scene/light.rs`), and native's ortho projection holds
   exact zeros where ours' decomposition left noise (`shading/view_space.rs`). Cascade constants
   went from 67 of 272 floats differing to the 5 every view has (time, random floats); cascade 2
   keeps 2 more below 1e-3. Contact shadows (`screen_space_shadows_cs`) run and the stand-in reads
   their mask (item 9).
3. **Depth format and stencil (fixed).** Main depth is `D32SfloatS8Uint` (image format 92,
   `swap_chain::DEPTH_IMAGE_FORMAT`; D24S8 where the device lacks the feature), sampled and read
   back through its depth aspect. The capture layer now records stencil ops, masks and reference
   (`sops`). Real's states, now ours (`render/depth.rs` `stencil`): prepasses `Equal`, compare
   mask 0x81, write mask 0, reference 0; opaque front faces `NotEqual`, mask 1, write mask 0xFE,
   reference 3, back faces `Always`. Every op is `Keep`, so nothing writes stencil past its clear
   to 0; only managed's overlay prepass would set bit 0x80. Left: real sets depth bias [0, 0, 0]
   with bias enabled, which wgpu can't express (zero bias is "off").
4. **Depth prepass (fixed).** Without `NeedsDepthNormals` (camera attributes now parent the view's,
   where managed reads it) the prepass has no colour target and draws through the depth-only
   stand-in, back faces culled, each mesh one instanced draw. `SetBoundingVolumeSizeCullThresholdInPercent`
   splits Large (≥ 60 % of screen height) from Small (`scene/collect.rs`). Managed's
   `SetOcclusionCull` buffers are filled before the layers run (`scene/occlusion.rs`): a run per
   draw with its layer's phase (early, late, final), a cull instance per drawn instance (world
   bounds; never culled with `NoOcclusionCulling`), arguments with no instances, the count where
   `D_GPU_COUNT` reads it, and the dispatch. Those draws go `drawIndexedIndirect` with the cull's
   surviving ids as the instance stream; managed's `occlusion_cull_cs` fills them. The late
   prepass draws only those. Showroom, skinned and outdoor now line up pass for pass with real's
   prepasses (draw counts, instance counts, x0 late draws). wgpu's indirect validation (a dispatch
   and an args copy per indirect pass) stays on in the engine, because managed game code fills
   those args. `gpu.sh` and `perf.sh` default `WGPU_VALIDATION_INDIRECT_CALL=0`, so captures match
   real's, which validates none. Pass `=1` to see it. Draws from the vertex cache and those with managed's instance ids stay direct, as real's
   skinned draws do. Opaque material draws aren't batched into instanced draws (real: one draw for
   showroom's three boxes). A depth-normal prepass (outdoor) still draws the material program
   with colour off; real writes RGBA normals.
5. **Vertex streams (fixed for static meshes).** Compiled meshes draw from a 20-byte copy as real's:
   position, `R16G16Sfloat` UV, `A2B10G10R10` packed normal and tangent (`gpu_mesh::draw_vertices`,
   assets `pack_frame`, the inverse of `unpack_frame`); slots are real's: mesh, instance ids
   (stride 4), defaults (stride 0, per instance). The stand-in decodes the frame as `vs_decompress`
   (UNORM input) and passes the tangent on. Left, skinned: real skins these meshes from the mesh's
   own float stream (`R32G32Sfloat` UV, float normal and tangent, `R8G8B8A8Unorm` colour, two
   `R32Sfloat`, `R8G8B8A8Uint` indices; the prepass reads position, the two floats, ids and
   indices); ours draws from the compute-skinned vertex cache.
6. **Constants ours left zero (fixed).** The 592-byte buffer now matches byte for byte: native's
   default wind (also what `GetSceneWorldWind` returns), the camera as
   `g_vHighPrecisionLightingOffsetWs`, and infinite fog culling bounds. Real's 80-byte cluster block
   comes from managed's `ClusteredCullingLayer` on the view's attributes. The material stand-in now
   walks the clusters as real's forward shading does (`ClusterCulling.hlsl`; lights and decals from
   the pixel's cluster), and scene draws in a render layer take the view's attributes beneath their
   own (`scene/execute.rs`), so its main-draw 80-byte block is byte-identical to real's (24×16×48,
   near 10, far 10000, 1024×768) and it reads managed's 73728-byte cluster lists. The 304-byte buffer
   is byte-identical. The 4128-byte light array holds NaN in real's unused slots, 0 on ours.
7. **Morph targets.** Real composites morphs in a 2048×1024 `R16G16B16A16Sfloat` pass of two additive
   draws before shadows; ours has no such pass (compute).
8. **Final output (mostly fixed).** A pass that neither clears nor draws isn't recorded (showroom:
   18 passes with the shadow views, real 21). `TargetSupportsSrgbWrite` is true for 8-bit targets, so managed's UI no
   longer takes the float path (the "UI Gamma Seed" / "Composite" blits through `R16G16B16A16Sfloat`);
   the last pass writes the swap chain image as real's does. Transfers are batched like real's
   (`render/batch.rs`): one upload arena per frame (per thread), written with one queue write per flush, buffer
   writes as ordered copies out of it, command buffers submitted together at present. Showroom 13
   (was 335), skinned 18 (was 745); real 16 and 14. The anti-aliasing composite has the depth test
   off on both sides; real's state still carries `GreaterOrEqual`, which wgpu can't set on a
   disabled test (it leaves `Never`), so `gpu_diff.py` ignores the op there. Left: the swap chain
   image is ours' own texture, copied to the surface at present. Engine textures are long-lived
   handles that binds, views, bindless entries and readbacks keep, while a wgpu surface texture
   lives for one frame and can't back one.
9. **Textures per draw.** Real binds material textures directly too (box: `primary_white_color`,
   plane: `testgrid_color`, the same files as ours); its one bindless array (about 1250 views, every
   loaded texture) serves shadow maps, the contact-shadow mask and decals. Ours fills a few dozen slots per
   view dimension and binds shadow maps directly; its copies of real's shadow indices select 1×1
   defaults. Decal textures pair by name on both (decals: star, ring, splotch, arrow, spark).
   Fixed:
   - Block-compressed textures upload as stored where the device samples BC (lavapipe does), mips
     included, created sRGB with a linear view as real's (`compiled/texture.rs` `stored`, decode
     the fallback). wgpu has no RGB-only BC1, so BC1 shows `Bc1RgbaSrgbBlock` against real's
     `Bc1RgbSrgbBlock`. Textures a `.vmat_c` names now load through the same path (they were
     linear images with mip 0 only).
   - The stand-in reads what real's main draw does: `g_tAmbientOcclusion` (ambient), `g_tBentNormal`
     (indirect normal under `F_USE_BENT_NORMALS`; complex's default set gains `default_bentnormal`,
     as real's core `default.vmat_c`), the cluster env maps times `BRDFLookup`
     (vr_lighting.fxc's specular loop; no corpus scene has env maps), and the contact mask by
     `g_DirectionalLightScreenSpaceShadowIndex` through `MSAAUtils::SampleRed`, which binds
     `g_tDepthChain`. The LUT is core's `ggx_integrate_brdf_lut_schlick.vtex` that managed's
     `ViewSetup.SetLookups` loads, not GPU-generated. The chain is `R32G32Sfloat` behind a
     filtering sampler: `FLOAT32_FILTERABLE`.
   - Contact shadows run: managed makes the mask `ImageFormat.A8`, now `R8Unorm` with storage.
   - Defaults as real's: an unset cube is a 1×1 transparent-black sRGB cube; no fog volume is a
     2×2×2 opaque-black sRGB image through a linear view.

   Left: real's main draw reads a 512×3072 `R16G16B16A16Unorm` table (almost certainly
   vr_lighting.fxc's `BlueNoise`, read only under `D_OPAQUE_FADE`; not in core, so not
   reproducible) and `materials/black_matte_vmat_g.tcolor` (a material's generated colour);
   skinned's crate draw also reads a 32×32 `R8G8B8A8Srgb` texture. The depth-only prepass binds no textures on either side, skinned's
   vertex-cache draws included; a depth-normal prepass (outdoor) binds the material stand-in's
   set, real's none (item 4). gpu_tex names project textures by label only, so skinned's crate
   doesn't pair by name.

Next steps: name real's shaders (their SPIR-V carries no source string) by matching stage interfaces.
