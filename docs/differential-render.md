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

Metrics (mean abs error R/G/B, SSIM) before the fixes, after 1–2, and after 3 and 5–7 (skinned
and glass: after their content fixes, below):

| Scene | Before | After 1–2 | After 3, 5–7 |
| --- | --- | --- | --- |
| showroom | 18.9 / 19.0 / 19.9, 0.892 | 1.9 / 1.6 / 1.1, 0.995 | 0.11 / 0.34 / 0.42, 0.999 |
| outdoor | 8.2 / 7.2 / 5.3, 0.978 | 3.0 / 2.7 / 2.4, 0.998 | 0.11 / 0.13 / 0.12, 0.999 |
| fog | 20.3 / 17.5 / 17.4, 0.901 | 5.0 / 4.8 / 8.0, 0.969 | 0.23 / 0.27 / 0.32, 0.997 |
| skinned | 12.2 / 11.5 / 14.7, 0.877 (real: error signs) | | 1.7 / 1.2 / 0.8, 0.980 |
| glass | no settled frame (real: error checker) | | 3.9 / 5.4 / 5.1, 0.980 |
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
5. **Fixed. Point light hotspot dimmer on ours.** Showroom lamp peak was (209,112,94) against
   real's (246,147,110); now both (247,148,111). The hotspot is specular, which the stand-in for
   closed `complex` lacked: it now lights as `vr_lighting.fxc` does, normalized pow(N·L) diffuse
   plus GGX specular, with roughness from `g_tNormal`'s blue channel (core's default normal map
   holds 0.5) and F0 0.04 (`shading/standin_lighting.hlsl`). Falloff and packing matched already;
   a point light now also packs native's zero axes and (0, -1, 1, 0) cone, and the spot cone
   factor is left unclamped as real's is (`lighting/blocks.rs`).
6. **Fixed. 3D skybox through distance fog brighter on ours** (outdoor: +8 to +13 per channel)
   with a seam at x ≈ 375 between two faces of a skybox box. Not the fog constants: the skybox's
   child view took the camera's ambient colour (and, with none, the stand-in's fallback sun).
   Real lights it with the skybox world's own lights only, so a skybox without one is black under
   its fog (`render/scene/skybox.rs`, `Lighting::of_worlds`).
7. **Fixed. Minification.** Grid lines were black on ours: ours uploaded mip 0 only. Compiled
   textures now upload every stored mip (`compiled/texture.rs`), and a vfx `Filter( ANISO )`
   sampler without a managed state gets anisotropy 4, the Low profile's `r_max_anisotropy`
   (`shading/defaults.rs`; mips alone left far lines too bright). Mean luma of showroom's grid
   line pixels, ours/real: near floor 35.7/43.3 → 43.5/43.3, far floor 75.6/78.6 → 78.9/78.6.
8. **Low. Banding.** Concentric rings in the diff around light pools are 1-LSB offsets: along a
   gradient both frames step through the same number of levels (no dithering on real), with
   boundaries a pixel or two apart. Shadow edges differ by a pixel too. Not pursued.
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

- **Rendering (9, 10):** sequence autoplay; translucent opacity. The texture quality group's
  convars (`r_texture_stream_*`, `r_max_anisotropy`) live in real's rendersystem and aren't
  defined yet; managed warns once and skips them. Define them, then read `r_max_anisotropy`
  instead of the fixed 4. The froxel grid stays 96×56×64 whatever `volume_fog_*` say.
- **Content:** seed real's compiled `.vmdl_c` into ours (`frame.sh`) now that primitive types read.
- **HUD (4):** keep real's `FrameStart` alive under `-nosteam` without going online.
