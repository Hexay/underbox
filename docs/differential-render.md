# Rendering differentials against real engine2

Part of docs/differential.md (its rules apply). `run.sh frame` boots both clients on a corpus
scene at 1024×768 on lavapipe. Real needs two of its own switches: `-nosteam` (else it waits
forever for a Steam client) and `-vulkan_allow_cpu` (else "No usable Vulkan physical devices were
found": it skips CPU device types). Its window sits at the origin of a screen-sized Xvfb; four
identical grabs one second apart count as settled. Ours is seeded with real's compiled core
textures and materials (`.vtex_c`, `.vmat_c`; shaders stay each engine's own), since sbox-public
ships none of core's images. Both pin shadow quality High and volumetric fog Off (`FOG_QUALITY`,
the auto-detected Low preset's level; the goldens pin High); the rest of the Low preset applies to
both. Never commit real's frames.

Metrics (mean abs error R/G/B, SSIM) before the fixes, after 1–2, after 3 and 6 (skinned and glass:
after their content fixes, below):

| Scene | Before | After 1–2 | After 3, 6 |
| --- | --- | --- | --- |
| showroom | 18.9 / 19.0 / 19.9, 0.892 | 1.9 / 1.6 / 1.1, 0.995 | |
| outdoor | 8.2 / 7.2 / 5.3, 0.978 | 3.0 / 2.7 / 2.4, 0.998 | 0.15 / 0.12 / 0.11, 0.999 |
| fog | 20.3 / 17.5 / 17.4, 0.901 | 5.0 / 4.8 / 8.0, 0.969 | 1.4 / 1.3 / 1.0, 0.977 |
| skinned | 12.2 / 11.5 / 14.7, 0.877 (real: error signs) | | 2.7 / 1.8 / 0.9, 0.976 |
| glass | no settled frame (real: error checker) | | 4.8 / 5.8 / 5.1, 0.977 |
| hud | 37.7 / 29.4 / 36.4, 0.851 | | open, 4 |

Ranked divergences:

1. **Fixed. Real's compiled textures didn't load on ours.** Real stores mips zstd-compressed
   (method 2 in the compressed-mip entry, which declares only its 12-byte head; the size array
   follows it). Ours read only LZ4 and failed with "mip 0 runs past the end". A source `.vmat`
   naming `x.tga` now also finds the compiler's `x_tga_<hash>.generated.vtex_c` beside it
   (`compiled/texture.rs`). All 689 core textures read (`real_core_textures_read`, with
   `UNDERBOX_REAL_CORE`).
2. **Fixed. The dev plane's texture was rotated 90°.** Real maps `RenderPrimitivePlane` U along +Y
   and V along +X (`model/primitive.rs`). Ours' stand-in testgrid hid both 1 and 2.
3. **Fixed. Volumetric fog drawn though the settings turn it off.** The Low preset stores
   `volumetricfog.quality -1`, so `quality_profiles.json` sets `r_enable_volume_fog 0`; managed
   dropped the write because ours announced no native convars. `render/convars.rs` now defines
   real libengine2's (`r_3d_skybox`, `r_enable_volume_fog`, `volume_fog_width/height/depth`, with
   its defaults 1, 1, 240, 160, 64), `SourceEngineInit` announces them before `ApplyAll`, and
   `fog_volume::render` draws nothing while `r_enable_volume_fog` is 0. Left in fog: the roof's
   shadow edge across the box (real's is a soft band) and the foreground shadow (ours 0, real 12).
4. **Open, harness: real draws no HUD.** Under `-nosteam` `SteamFriends()` is null, so
   `SteamRichPresenceSystem.Poll` throws every frame (from `EngineLoop.FrameStart` via
   `GameInstanceDll.Tick`); `RunFrame` catches it and skips the rest of `FrameStart`,
   `SimulateUI()` included. Managed loads its own `libsteam_api.so` (`NativeDllPath`), so
   `LD_PRELOAD` can't stub Steam; hot-patched Friends/Utils got past the throw, but the client
   then called the sbox API (401) and never loaded the game.
5. **Point light hotspot dimmer on ours.** Showroom lamp peak (209,112,94) against real's
   (246,147,110); sun-lit and shadowed floor agree within 2. Guess: point light falloff or
   specular in `render/lighting/`.
6. **Fixed. 3D skybox through distance fog brighter on ours** (outdoor: +8 to +13 per channel)
   with a seam at x ≈ 375 between two faces of a skybox box. Not the fog constants: the skybox's
   child view took the camera's ambient colour (and, with none, the stand-in's fallback sun).
   Real lights it with the skybox world's own lights only, so a skybox without one is black under
   its fog (`render/scene/skybox.rs`, `Lighting::of_worlds`).
7. **Minification.** Grid lines are black on ours (0) and grey on real (18): ours uploads mip 0
   only (`compiled/texture.rs`), real samples its mip chain.
8. **Low. Banding.** Concentric rings in the diff around light pools: 8-bit quantisation differs
   (real likely dithers).
9. **Puppet pose** (skinned). Ours plays `bow` (top face lifted and twisted by bone and morph);
   real holds the bind pose. Guess: ours autoplays the first sequence on a renderer without an
   anim graph, and real doesn't.
10. **Translucent glass lighter on ours** (glass): the whole pane and the box behind it are
    lighter and more cyan on ours, and ours draws a specular edge real lacks. Guess:
    translucent opacity (`g_flOpacityScale`, tint alpha) or translucent lighting.
11. **Low. Crate tops lighter on ours** (skinned).

Skinned and glass were content confounds: real rejected our synthetic `.vmdl_c` (error signs)
and had no `glass.vmat_c` (error checker). What real needs from our compiled files, found by
bisecting them: research/resources.md "What real engine2 accepts". `real-game.sh` now runs
`assetc --materials` because real only compiles `.vmat` sources in the editor.

Agreeing: sky clear colour (exact), ACES auto-exposure on outdoor's floor (exact), sun shadows'
placement and softness, box shading within 1–6.

## Follow-up lanes

- **Rendering (5, 7, 9, 10):** point light falloff; mips; sequence autoplay; translucent opacity.
  The texture quality group's convars (`r_texture_stream_*`, `r_max_anisotropy`) live in real's
  rendersystem and aren't defined yet; managed warns once and skips them. The froxel grid stays
  96×56×64 whatever `volume_fog_*` say.
- **Content:** seed real's compiled `.vmdl_c` into ours (`frame.sh`) now that primitive types read.
- **HUD (4):** keep real's `FrameStart` alive under `-nosteam` without going online.
