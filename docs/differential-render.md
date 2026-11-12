# Rendering differentials against real engine2

Part of docs/differential.md (its rules apply). `run.sh frame` boots both clients on a corpus
scene at 1024×768 on lavapipe. Real needs two of its own switches: `-nosteam` (else it waits
forever for a Steam client) and `-vulkan_allow_cpu` (else "No usable Vulkan physical devices were
found": it skips CPU device types). Its window sits at the origin of a screen-sized Xvfb; four
identical grabs one second apart count as settled. Ours is seeded with real's compiled core
textures, materials and models (`.vtex_c`, `.vmat_c`, `.vmdl_c`, `.vmesh_c`; shaders stay each
engine's own), since sbox-public ships none of core's images. Both pin shadow quality High and volumetric fog Off (`FOG_QUALITY`,
the auto-detected Low preset's level; the goldens pin High); the rest of the Low preset applies to
both. `MANAGED=1` runs both through managed's scene renderer (`+r_managed_scene 1`) into
`target/diff/frame/<corpus>-managed`, for divergences only the managed path shows. Never commit
real's frames.

Metrics (mean abs error R/G/B, SSIM) before the fixes, after 1–2, and after 3 and 5–7 (skinned
and glass: after their content fixes, below):

| Scene | Before | After 1–2 | After 3, 5–7 |
| --- | --- | --- | --- |
| showroom | 18.9 / 19.0 / 19.9, 0.892 | 1.9 / 1.6 / 1.1, 0.995 | 0.11 / 0.34 / 0.42, 0.999 |
| outdoor | 8.2 / 7.2 / 5.3, 0.978 | 3.0 / 2.7 / 2.4, 0.998 | 0.11 / 0.13 / 0.12, 0.999 |
| fog | 20.3 / 17.5 / 17.4, 0.901 | 5.0 / 4.8 / 8.0, 0.969 | 0.23 / 0.27 / 0.32, 0.997 |
| skinned | 12.2 / 11.5 / 14.7, 0.877 (real: error signs) | | 1.7 / 1.2 / 0.8, 0.980 |
| glass | no settled frame (real: error checker) | | 3.9 / 5.4 / 5.1, 0.980 |
| hud | 37.7 / 29.4 / 36.4, 0.851 | | 0.02 / 0.02 / 0.01, 1.0000 (4) |

After 9–11 (`MANAGED=1` for managed):

| Scene | Native | Managed |
| --- | --- | --- |
| skinned | 1.26 / 1.23 / 0.83, 0.983 | 1.32 / 1.29 / 0.56, 0.983 |
| glass | 0.09 / 0.32 / 0.37, 0.9996 | 0.17 / 0.40 / 0.19, 0.9985 |

After 12–13, with real's core models seeded (native unless marked):

| Scene | Metrics |
| --- | --- |
| skinned | 0.26 / 0.59 / 0.65, 0.9955; managed 0.57 / 0.62 / 0.39, 0.9958 |
| showroom | 0.12 / 0.33 / 0.41, 0.9989 |
| outdoor | 0.10 / 0.11 / 0.11, 0.9994 |
| fog | 0.26 / 0.24 / 0.26, 0.9972 |
| glass | 0.09 / 0.27 / 0.34, 0.9996 |

After 14–15 (skinned with its third, morphed puppet; the others unchanged where not listed):

| Scene | Metrics |
| --- | --- |
| skinned | 0.17 / 0.31 / 0.47, 0.9984; managed: real draws no morphed puppet (15) |
| glass, managed | 0.09 / 0.27 / 0.07, 0.9997 (0.17 / 0.40 / 0.19, 0.9985 after 9–11) |

Shadows through managed's shadow mapper and core's sun filter (docs/differential-gpu.md finding
2, docs/lighting.md), same real frames:

| Scene | Before | After |
| --- | --- | --- |
| showroom | 0.14 / 0.33 / 0.41, 0.9989 | 0.12 / 0.31 / 0.40, 0.9993 |
| outdoor | 0.10 / 0.11 / 0.11, 0.9994 | 0.10 / 0.11 / 0.11, 0.9994 |
| fog | 0.25 / 0.24 / 0.26, 0.9972 | 0.14 / 0.17 / 0.22, 0.9991 |
| skinned | 0.18 / 0.32 / 0.47, 0.9983 | 0.12 / 0.26 / 0.43, 0.9997 |
| glass | 0.11 / 0.27 / 0.35, 0.9996 | 0.10 / 0.26 / 0.35, 0.9998 |

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
   `fog_volume::render` draws nothing while `r_enable_volume_fog` is 0. The roof's soft
   shadow band across the box now matches (core's tent filter, docs/lighting.md).
4. **Fixed, harness: real drew no HUD.** Under `-nosteam` `SteamFriends()` is null, so
   `SteamRichPresenceSystem.Poll` throws every frame (from `EngineLoop.FrameStart` via
   `GameInstanceDll.Tick`); `RunFrame` catches it and skips the rest of `FrameStart`,
   `SimulateUI()` included. `real-client.sh` now runs with the friends stand-in and shim, the web
   API cut off, and a newer lavapipe (Mesa 26.0 crashed in real's UI shader): docs/differential.md,
   "Real's UI on lavapipe". Real's HUD then matches ours: every box, border, radius, gradient and
   both labels, mean error under 0.03 per channel, SSIM 1.0000. Showroom under the new setup:
   0.14 / 0.33 / 0.41, 0.9989 (before 0.12 / 0.33 / 0.41).
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
   its fog (`render/scene/skybox.rs`, `Lighting::of_worlds`). The managed path keeps a seam at
   x ≈ 365 and that is correct: Sandbox.SceneRenderer binds every lighting block itself and gives
   its skybox system the main world's ambient (`RenderSystem.Skybox.cs`, `ViewSpace.Lighting`) and
   no sun, so the two skybox boxes show their tints. Real under `MANAGED=1 run.sh frame outdoor`
   draws the same seam: 0.11 / 0.13 / 0.16, SSIM 0.9994.
7. **Fixed. Minification.** Grid lines were black on ours: ours uploaded mip 0 only. Compiled
   textures now upload every stored mip (`compiled/texture.rs`), and a vfx `Filter( ANISO )`
   sampler without a managed state gets `r_max_anisotropy` (`shading/defaults.rs`; real's
   default and the Low profile's are both 4; mips alone left far lines too bright). Mean luma of showroom's grid
   line pixels, ours/real: near floor 35.7/43.3 → 43.5/43.3, far floor 75.6/78.6 → 78.9/78.6.
8. **Low. Banding.** Concentric rings in the diff around light pools are 1-LSB offsets: along a
   gradient both frames step through the same number of levels (no dithering on real), with
   boundaries a pixel or two apart. Shadow edges differ by a pixel too. Not pursued.
9. **Fixed. Puppet pose** (skinned). Real played `bow` too (the sequence advanced) but held the
   bind pose: our `.vmdl_c` was the cause, not autoplay. Bone flags, decoder table, quaternion
   segments, block overlap, mesh inverse binds and unorm8 weights: research/resources.md "What
   real engine2 accepts". The split weights left here: 12.
10. **Fixed. Translucent glass lighter on ours** (glass). Three causes. `complex` takes
    `g_vColorTint` in gamma space (`SrgbGammaToLinear`, `vr_shared_standard_vs_code.fxc`; the
    stand-in used it as linear). Real's compiler binds `materials/default/default_color.tga`
    (grey 187) for an unset colour, where ours had white (`material_source.rs`,
    `texture_generated.rs`). Real's translucent materials cast no shadow, native or managed
    (`draw/model.rs` `shadow_draws`, `draw/draw_call.rs`).
11. **Fixed. Crate tops lighter on ours** (skinned): a compiled draw call's `m_vTintColor` and
    `m_flAlpha` multiply the object's tint on real; ours dropped them (`DrawCall::tinted`; managed
    draws pass it to the stand-in as `StandinDrawTint`).
12. **Fixed, content. Split skin weights** (skinned). A puppet side vertex weighted half spine,
    half root followed the spine fully on real. Bisected with per-face fixture variants: with
    blend indices (root, spine) the vertex followed the root, with (spine, head) the spine, so
    real used the first weight only. Not the bone flags (real's compiler adds 0x80 to bones a
    mesh weights; ours now does too, no change) but the morph set: without it real blends exactly
    as ours does. Before: 1.26 / 1.23 / 0.83, 0.983 (managed 1.32 / 1.29 / 0.56, 0.983). The
    morph set's cause is 15; the open puppet's inside faces 14.
14. **Fixed. Inside faces of the bowed puppet** (skinned). Ours drew them, real culls them. Our
    material stand-in declared no `CullMode`, so it drew both sides; real's closed shaders cull
    back faces unless the material sets `F_RENDER_BACKFACES` (vr_common_ps_code.fxc's
    `MATERIAL_CONTROLLED_BACKFACES`). The stand-in now does the same (`Shaded::raster_state`).
    Native 0.26 / 0.59 / 0.65, 0.9955 → 0.15 / 0.28 / 0.46, 0.9990; managed 0.57 / 0.62 /
    0.39, 0.9958 → 0.46 / 0.31 / 0.20, 0.9992.
15. **Fixed natively, content. Morphed meshes skinned by their first weight on real.** Real's
    citizen model (fetched for this) names each mesh's atlas in CTRL as well as in MRPH:
    `embedded_meshes[].morph_texture`. Ours wrote it only in MRPH; with the CTRL entry real
    blends every weight. Bisected on the skinned scene's third puppet, `puppet_morphed` (smile
    ramps over "bow"): without `morph_texture` 0.68 / 0.70 / 0.73, 0.9907 (its sides follow the
    spine fully); with it 0.17 / 0.31 / 0.47, 0.9984. MRPH's `m_nLookupType` and
    `m_nEncodingType`, which ours now writes as real does, made no difference. **Open:** through
    managed's renderer real still draws no morphed puppet at all (managed 0.97 / 1.25 / 1.09,
    0.9887), and logs nothing about it.
13. **Fixed. Real's compiled models went dark on ours.** Real's core models store `NORMAL` as
    `R10G10B10A2_UNORM` (format 24), which ours didn't decode at all (zero normals). skinning_cs
    shows only the split (x, y, z 10 bits, w 2); `vs_decompress.fxc` is closed, so the decode was
    fitted to 28k vertices of real's core models against their face normals and UV tangents: x, y
    an octahedral normal; z a diamond-encoded tangent direction in Duff et al.'s basis around it;
    w 3 for bitangent sign +1, 0 for −1 (`compiled/vertex.rs` `unpack_frame`, the stand-in's
    `_DecompressNormalTangent` the same). Normals match to 0.9999, tangents to 0.98: the rest is
    real's compiler storing a tangent exactly opposite the basis axis as the axis itself
    (`real_core_tangent_frames_match_their_geometry`, with `UNDERBOX_REAL_CORE`). `frame.sh` now
    seeds real's `.vmdl_c` and `.vmesh_c`.

16. **Fixed. Decals weren't drawn on ours** (decals). `CDecalSceneObject` was bookkeeping only and
    the cluster's decal count was 0. Now `render/lighting/decals.rs` packs every rendering decal as
    native's `GPUDecal` and `GPUDecalExtraData` (managed's `LightBinnerFeature.Decals.cs` documents
    the packing: negated position, conjugate rotation, reciprocal scale, RGBE tint, sRGB colour
    index), sets `NumLights.w`, and the stand-in resolves them as Decals.hlsl does, every decal per
    pixel instead of the cluster walk (`shading/standin_decals.hlsl`). Before: 55.2 / 60.8 / 51.1,
    SSIM 0.754.
17. **Fixed. Managed decals drew a black frame** (decals, `MANAGED=1`). Two causes. The decal's
    texture getters returned null for unset textures and managed's `Texture.FromNative` threw every
    frame; they now hand out an empty handle or a copy (`scene/decal.rs`). And `GetTextureViewIndex`
    gave -1 for a null texture, which managed packs into 16 bits as 65535: the RMO sample read far
    outside the bindless array and lavapipe never finished the frame. Null is now 0, which every
    shader reads as "none" (`render/bindless.rs`). Before: 114 / 120 / 139, SSIM 0.0006.
18. **Agreeing. Particles** (particles). Additive glow sprites, sorted alpha-blended sprites with
    roll, and model particles (`ParticleModelRenderer`, shadowed) all match through managed's
    sprite compute path. The stock emitters draw from an unseeded random (`ParticleRandom` is
    seeded only by SceneLab), so the scene's `GridEmitter` (corpus code) emits a fixed grid with no
    velocity; real gives pixel-identical frames run to run on both scenes. Left: particle cubes'
    shadow edges and 1-LSB rings in the glows. A sheeted particle texture
    (`particle_debris_02`) draws nothing on either engine.
19. **Low. Decal edges.** Interiors match exactly (the star, ring, multiply splotch and additive
    spark); the alpha boundary sits up to a pixel apart. Not pursued.

After 16–17:

| Scene | Native | Managed |
| --- | --- | --- |
| particles | 0.29 / 0.47 / 0.21, 0.9983 | 0.27 / 0.45 / 0.18, 0.9986 |
| decals | 0.20 / 0.31 / 0.51, 0.9971 | 0.19 / 0.28 / 0.50, 0.9976 |

Reverse Z, real's per-view constants and the direct swap-chain output (docs/differential-gpu.md items
1, 6 and 8) moved no metric at two decimals. Measured on one pair of real frames, before and after:

| Scene | Before and after |
| --- | --- |
| showroom | 0.14 / 0.33 / 0.41, 0.9989 |
| outdoor | 0.10 / 0.11 / 0.11, 0.9994 |
| fog | 0.25 / 0.24 / 0.26, 0.9972 |
| skinned | 0.18 / 0.32 / 0.47, 0.9983 |
| glass | 0.11 / 0.27 / 0.35, 0.9996 |
| particles | 0.31 / 0.47 / 0.21, 0.9983 |
| decals | 0.25 / 0.32 / 0.53, 0.9957 |

Skinned and glass were content confounds: real rejected our synthetic `.vmdl_c` (error signs)
and had no `glass.vmat_c` (error checker). What real needs from our compiled files, found by
bisecting them: research/resources.md "What real engine2 accepts". `real-game.sh` now runs
`assetc --materials` because real only compiles `.vmat` sources in the editor.

Agreeing: sky clear colour (exact), ACES auto-exposure on outdoor's floor (exact), sun shadows'
placement and softness, box shading within 1–6.

## Real games: sbox-walker

`run.sh frame <game checkout>` runs a game instead of a corpus scene (docs/walker.md for the
game). `frame.sh` syncs the checkout into a scratch copy under `$UNDERBOX_GAME_COPIES`
(`tools/game/copy.sh`, as `tools/game.sh`); the checkout stays read-only. `prepare.py` starts
the copy in `SCENE` and injects `tools/diff/FramePin.cs`, which pins the camera
(`CAMERA_POS`, `CAMERA_ANGLES`, `CAMERA_FOV`), stops scene time at `FREEZE_AT` (default 0),
holds rigidbodies still, drops particle effects (unseeded emitters) and hides screen panels
(`KEEP_HUD=true` keeps them). `--stub-lobby` routes the game's `Networking.CreateLobby` to a
no-op on both engines: real segfaults in `SteamAPI_ISteamMatchmaking_CreateLobby` under
`-nosteam` (walker's `GameManager.OnHostInitialize`). Real compiles the copy with `assetc` as for
the corpus.

    SCENE=scenes/scenemap.scene CAMERA_POS=-260,-260,180 CAMERA_ANGLES=21,39,0 \
        tools/diff/run.sh frame ../sbox-walker

Walker's map plus `engine.scene` (camera with Tonemapping, Bloom, Sharpen, Vignette), host
player spawn skipped with the lobby. Neither side has walker's cloud content (`facepunch.*`
models and materials: coins, road cone, door, brick walls); both draw their error stand-ins.
Real is deterministic: two runs differ by 0.96 / 0.44 / 0.43, SSIM 0.9953, before rigidbodies
were held (the error-model road cone fell a frame-timed distance).

| Run | Metrics |
| --- | --- |
| First (ours with HUD) | 59.1 / 58.1 / 57.4, SSIM 0.301; luma ours 20.4, real 73.8 |
| After 7–9, HUD hidden | 56.8 / 60.2 / 56.1, SSIM 0.336; luma ours 19.7, real 73.9 |
| Same, `Sharpen` stripped from both (scratch copy) | 16.6 / 23.1 / 25.8, SSIM 0.749; luma ours 73.6, real 73.9 |
| After 1 and 3 (sRGB layer output, hoisted globals) | 13.9 / 20.1 / 23.1, SSIM 0.770; luma ours 72.9 |
| After 2, text placed (checker, identity matrices, instance transform) | 11.3 / 9.3 / 12.0, SSIM 0.790; luma ours 71.5 |
| After 4–5 (keyframed trigger, linear `generic` tint) | 6.6 / 4.8 / 4.9, SSIM 0.920; luma ours 71.7 |

Ranked divergences:

1. **Fixed, ours: `Sharpen` darkened the whole frame** (luma 19.7 against 73.9). It blits at
   `Stage.AfterPostProcess` onto the 8-bit target, and native writes a layer whose
   `SetOutput` colour space is sRGB through the target's sRGB view; ours wrote linear. Render
   and procedural layers and `RestoreRenderTargets` now carry `srgb_write`
   (`layer::srgb_output`), and an 8-bit target attaches its sRGB twin
   (`Texture::srgb_target_view`). Corpus goldens unchanged.
2. **Fixed, ours: the error material drew as plain lit grey.** `error.shader` (closed) maps to
   `standin::Kind::Error`: a world-space checker of 2-unit cells, its two reds fitted to real's
   run lengths and levels; a missing `.vmat` gets `error.shader` too.
3. **Fixed, ours: world text was missing** ("FLAMING MOES", `ui_text` with `D_WORLDPANEL` 1):
   `ui/scissor.hlsl` reads `BoxSize` before `ui/pixel.hlsl` declares it. vfx moves annotated
   globals into a cbuffer ahead of the code; `shaders::hoist` moves a global read above its
   declaration to the top. The draw sets no `LayerMat` or `TransformMat`; unset matrix
   attributes default to identity (`bind::uniform_value`), and a custom object's immediate draws
   take its world transform as the instance transform (`immediate::set_object`), as native's
   `GetTransformMatrix`.
4. **Fixed, physics: the floor moved on real, not on ours.** Top-down frames (camera
   `32,0,700`, angles `89,0,0`) show real's floor centred on `TeleportTarget` (4,200,4): the
   keyframed `TriggerTeleport` box rests on the static floor and real teleports the floor's
   root. Ours reported trigger pairs only with a dynamic body; now every trigger reports every
   pair but static-static, as real (`shape::pair_types`; docs/differential-tap.md, "Fixed"). That
   also explains the coins: rolled 180°, their error models hang below the old floor, half sunk on
   ours.
5. **Fixed, ours: error models were redder.** `materialforerrormodel.vmat` (`generic.shader`)
   tints by `[0.753 0.251 0]`; real's gold fits a linear tint, so `generic` maps to
   `Kind::Generic`, which skips the stand-in's gamma decode.
6. **Fixed, harness: real crashed creating walker's lobby** (above).
7. **Fixed. A pass sampling its own target dropped whole submits.** The bindless array bound
   every texture, the depth prepass's normals target included, and wgpu rejects an attachment
   that is also bound (64 validation errors a run; Vulkan only leaves it undefined). The pass's
   targets now get the default view in the array (`bindless::view`).
8. **Fixed. Mip-generation dispatches were skipped.** `Set( name, texture, mip )` binds that one
   mip natively; ours bound every mip, so `downsample_cs` read the mip it wrote (walker's
   backdrop blur). A texture attribute set for a mip now samples a one-mip view
   (`attributes::texture_slice`, `Texture::mip_sample_view`).
9. **Fixed. Missing models drew the cube stand-in.** Native draws core's
   `models/dev/error.vmdl`; ours now does too, compiled when seeded, else from source
   (`model.rs` `error_content`).
10. **Open, HUD:** with `KEEP_HUD=true` (7.1 / 5.9 / 5.7, SSIM 0.918) the `Scoreboard` overlay no
    longer sticks on ours. Walker's coin counter draws grey outlined icons on ours where real
    draws filled glowing dots, and the score digit sits about 10 px lower on ours.

## Follow-up lanes

- **Walker (above):** the HUD's coin icons and score position (10).

- **Rendering:** `render/convars.rs` defines the
  texture quality group's convars with real's defaults (`r_max_anisotropy` 4,
  `r_texture_stream_max_resolution` -1, `r_texture_stream_resolution_bias_min` 1.0). Nothing
  reads the streaming two yet, and managed sampler states don't clamp to `r_max_anisotropy`
  (`render/samplers.rs`), though real clamps them. The froxel grid stays 96×56×64 whatever `volume_fog_*` say.
- **Content:** why real's managed renderer draws no morphed puppet (15). Next: does real draw
  its own citizen under `MANAGED=1` on lavapipe? If not, it's real's, not our content.
- **Decals (16):** normal and height (parallax) maps, sheeted decal textures
  (`GetSheetSequenceTextureData`), and the exclusion mask aren't applied; the corpus uses none.
  Decal edges (19).
