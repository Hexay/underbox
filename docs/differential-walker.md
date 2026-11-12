# Frame differentials on a real game: sbox-walker

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
