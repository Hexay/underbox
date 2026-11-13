# Real games: rendering and shaders

Split from docs/real-games.md.

## Rendering

Fixed, in the order they showed up after spawn:

- **GPU hang in `sprite_cs`.** Managed `SpriteData` is packed (`Vector3` is 12 bytes), but DXC's
  `-fvk-use-gl-layout` lays structured buffers out as std430: `Rotation` lands at 16, `Splots` at
  80 instead of 68, stride 176 instead of 148. `Splots` read a float's bits, so the splot loop
  ran billions of times. `shaders/src/packed_buffers.rs` rewrites any structured buffer whose
  struct std430 would pad into packed `uint` words. The same mismatch was in every lit shader's
  light, shadow, DDGI and decal buffers, the UI box and text buffers, `line_cs` and the terrain
  brush (`crates/shaders/tests/packed_audit.rs` lists them).
- **Hangs in general** now log `wgpu error: GPU hang` and drop work until the queue drains
  (docs/renderer.md, GPU hangs).
- **Uniform grey frame.** The first-person arms and physgun are vmdl-only, so they are 16-unit
  error cubes centred on the eye, and the placeholder drew their inside faces over the whole
  view. The placeholder now culls back faces as native's default material does.

- **No HUD.** Six causes, all reproduced on `corpus/hud` (panels, borders, radii, a gradient,
  text; goldens on both paths): `RestoreRenderTargets` unbound the targets; PainterBatcher's
  vertex-less `DrawIndexedInstanced` was dropped; managed GpuBuffers lacked `INDEX` usage;
  `g_vViewport < Source( Viewport ) >` and the `LayerMat`/`TransformMat` matrix attributes were
  never bound, so every quad collapsed; draws didn't see the layer's `UIGammaOutput`, so colours
  were decoded twice; and `PainterTextInstances`' struct is renamed by `#define`, so the
  packed-buffer pass missed it and only the first glyph drew.
- **Grubs terrain:** `gr_foreground*` use a border-addressed sampler, invalid without
  `ADDRESS_MODE_CLAMP_TO_BORDER`; the device now asks for it and falls back to clamp-to-edge.
- **Grubs water:** `new_water.shader` redefines `SrgbGammaToLinear`, which our built-in
  `math_general.fxc` also defines (built-ins now yield to a same-signature definition), and its
  source is newer than its `.shader_c` (reads `i.vPositionPs` in PS, which current core doesn't
  declare). It draws with the `.shader_c`'s HLSL, as native does.
- **`downsample_spd`** writes managed's Rg32Float pyramid: the program is rebuilt with the bound
  texture's storage format.
- **Blue-white haze in the join frame** (host frames too): the physgun viewmodel's additive
  sprites (`light_glow_03`, `particle_flare_7b`, `lightcookies_09`) have only `.vtex` JSON in
  sbox-public, no image, so they sampled the white default as large hard squares a few units from
  the eye. Core's `textures/particles/flares/` and `textures/lightcookies/` now get a generated
  soft round glow (`render/texture_generated.rs`). Found with `UNDERBOX_LAYERS=probe`: only the
  translucent layers changed.
- **Combos from the draw:** graphics programs were always built at combo 0, so sprites'
  additive `D_BLEND` drew alpha-blended (dark rims) and `D_TEXT` was ignored. Programs now build
  at the combos the draw's attributes set (`shading::program`).
- **CMAA2** blends, the same every frame. Its blend-list heads (`RWTexture2D<uint>`) are cleared
  by plain writes in pass 0, pushed onto with `InterlockedExchange` in pass 2 and walked by plain
  reads in pass 3. Lowering those atomics to image read/write lost exchanges between threads, so
  threaded lavapipe dropped different blend items each frame (up to ~70/255 on edge pixels). Real
  atomics on a word buffer fixed that but cut pass 2 off from the image the other passes use:
  pass 2 chained onto last frame's stale heads and pass 3 saw only empty lists, so CMAA2 did
  nothing. The renderer now copies the image into the words before a dispatch and back after
  (`shading/texel_words.rs`). `compute/cmaa2_tests.rs` runs core's `cmaa2_cs` the way
  `Cmaa2Renderer` dispatches it and checks that it blends and repeats exactly. Golden frames
  include it again.

What's left, and why:

- No textures, no sky texture: the zoo's materials are `reflectivity_30.vmat` and
  `skybox_day_01.vmat`, whose `.tga`/`.exr` aren't in the repo and no `.vtex_c` ships. The sky is
  our flat placeholder. Grubs' `rocks_a_*` textures are absent from its checkout too.
- The one spotlight is the `studio_light_projector` prop's; the scene itself has only the sun.
- Grubs' input hints show words (`arrow_drop_up`, `arrow_left`) instead of icons. That's a
  content gap, not a renderer bug. Core's `styles/base/rootpanel.scss` sets `IconPanel, i` to
  `font-family: Material Icons`, a ligature font. Managed's `FontManager` loads fonts only from
  `/fonts/*.ttf|otf` in the mounted file systems. sbox-public's core has no `fonts/` folder (no
  Roboto or Inter either), Grubs ships only Londrina Solid and Passion One, and this machine has
  no icon font. So `TypefaceFromStyle` logs `Font 'Material Icons' not found` and falls back to
  SkiaSharp's system default, which draws the ligature names as text. The font is Apache-2.0
  (sbox-public's `thirdpartylegalnotices/licenses/material-design-icons`), but it ships with
  s&box's Steam content, which we don't download. A user-supplied `MaterialIcons-Regular.ttf`
  under a mounted `fonts/` folder would load unchanged.

## Shaders

All 28 `.shader_c` files in both checkouts were checked with
`UNDERBOX_REAL_SHADERS=<dirs> cargo test -p engine2 --lib real_shader -- --nocapture`.

Confirmed:
- Container header version 12. Resource version 2, 3 or 4. Blocks: `GUID` (16 bytes, newer
  files), `REDI`, an empty `DATA`, then `SPRV` and `HLSL`.
- `SPRV` is 9 × `{u32 offset, u32 size}`, with offsets from the payload start and no length
  prefix. This disproves research/shaders.md's length prefix. Our reader already read it this way.
- Every program body starts `vcs2`, then the VCS version (65 or 68), then ABI 2.
- `HLSL` is `{"Programs": {"VFX_PROGRAM_VS": src, ...}}`: one source per program, keyed by name.
- Native masking blanks the block keywords (`MODES`, `FEATURES`, `COMMON`, `VS {`) to spaces and
  keeps line numbers. It keeps `#if (PROGRAM == ...)`. Each source starts with `#line 1` and
  `#include "system.fxc"`. Our own masking keeps the keywords. The loader now reads bare HLSL as
  that stage's block (`shader/load.rs`).
- The FEATURE body's feature records hold a 64-byte name, then a 64-byte group, then i32s. Max sits
  at +136 (VCS 65) or +140 (VCS 68). Embedded loads take their `FEATURES` from these records,
  because native blanked the block (`shader/native_features.rs`).
- Each stage body holds: the VCS header, a 16-byte program hash, a 16-byte hash shared across
  shaders, reflection text (attribute and semantic names), then combos. Each combo is `i32 -3`,
  two u32s, then a zstd frame.

Exceptions: `simple_water.shader_c` has a `DXBC` block and no `SPRV`, so it fails cleanly with "no
SPRV block". `triplanar_scorch.shader_c` has `SPRV` but no `HLSL`, so it can't load.

**PLAN.md's SPIR-V question:** yes, the bodies hold SPIR-V combos, but it can't be used. All 124
zstd frames need dictionary `0x2bc2fa87`, which no file carries; native ships it. So the shipped
bytecode can't be decoded or verified, and the embedded HLSL, compiled by DXC, is the usable path.
Core shaders have no `HLSL` block, but we compile those from source anyway.

Synthetic fixtures with the real layout live in `shader/compiled_tests.rs`. No real file is
copied.
