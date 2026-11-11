# Real games on underbox

First runs of third-party games: Facepunch/sandbox and apetavern/grubs (both MIT), read from
checkouts outside this repo. Nothing from them is copied, committed or downloaded; tests that
read their files take the folders from an env var and skip without it.

## Running

`tools/game.sh <checkout> [scene]` (optional, not in verify.sh). See its header for modes.

- Runs happen in a reflinked copy under `/srv/agents/cache/underbox-real-games/`. The copy gets
  `tools/game/RealGameWatch.cs` (a component logging `GAME:*` markers) and the startup scene, with
  `PackageReferences` and `MapList` emptied (`tools/game/prepare.py`).
- Package downloads stay off: the game process gets `HTTP(S)_PROXY=http://127.0.0.1:9`. Every
  managed `HttpClient` uses the default proxy, so sbox.game requests fail with "Connection
  refused" and packages log "Package not found". User namespaces aren't available here, so
  `unshare -n` isn't an option.
- `COMPILED_ONLY=1` deletes `.geq/.sdflayer/.surface/.vsnap` sources from the copy. engine2
  recompiles project JSON resources at mount, which would otherwise replace the shipped `_c` files.

Sandbox HEAD `397952f3` doesn't compile against sbox-public `2643ebd6`: `PlayerController` has no
`ShowArmsInFirstPerson` / `UseFirstPersonAnimation`. Its parent `6322a2a5` compiles. Export it
with `git -C /srv/agents/work/sandbox archive 6322a2a5 | tar -x -C <cache dir>`.

## What runs

| Milestone | State | Evidence |
|---|---|---|
| Zoo, dedicated server | passes | 0 compile errors, no missing components, no stubs hit. 15 bodies: 12 settle, 3 fall forever. The origin trace hits the floor. `GAME:END` reached in about 37 s. |
| Stand on the floor | passes (host side) | A PlayerController dropped from 64 units reports `GAME:STANDING on=Block v=0`. |
| Client joins | blocked without a patch | See [Join](#join). |
| Client renders the zoo | partly | Frames render until the player spawns. Then the GPU hangs in `sprite_cs`. See [Client frame](#client-frame). |
| Shipped `.shader_c` | read | 26 of 28 files load from their embedded HLSL. Every VS and PS compiles with DXC. See [Shaders](#shaders). |
| Grubs | partly | Code compiles. `beach.scene` loads on a server and on a host client. All 40 `.geq_c/.sdflayer_c/.surface_c/.vsnap_c` read. No round starts, so no terrain gets built. |

### Falling props

`washing_machine`, `tv` and `oil_drum_explosive` fall. Their models come from packages
(`models/sbox_props/...`, `models/props/...`), which stay refused. So they get the error model,
and a ModelCollider on it has no shapes. The checkout also holds only `.vmdl` sources for its own
models. Model compiling isn't ours (closed), so all 15 to 20 renderers show error models.

### Join

The client connects over our transport and the handshake works. It then stops at "Package
facepunch.sandbox wasn't found!" and disconnects cleanly.

- A dedicated server announces its game as `facepunch.sandbox` (`GetIdent(false, true)`, no
  `#local`).
- The client calls `Package.FetchAsync`, which the proxy refuses. `FetchOffline` then has nothing.
- The developer-host shortcut is `IsDeveloperHost = Application.IsEditor` only.
- Adding `#local` to the server's ident hits an NRE in `LocalPackage.UpdateFromPackage`
  (`TryGetCached` returns true with a null package). An org of `local` doesn't help either:
  `FetchAsync` returns null for it.

Getting past this needs a managed patch, such as engine-shim's `IsDeveloperHost`, or the published
package. Neither is allowed here, so it stops at this point.

### Client frame

`MODE=render` hosts the zoo in a headless client on lavapipe.

What the frame shows:
- A pale grey floor and pale grey walls, with no textures.
- A row of white error-model boxes with contact shadows.
- One spotlight at the left edge.
- No sky.

Every model is the error model, as described in [Falling props](#falling-props).

Two bugs appeared once the player spawned:

1. **Fixed:** managed `Texture.Clear` on a plain (non-render-target) texture became a clear pass
   on a view without `RENDER_ATTACHMENT`, which failed wgpu validation. Plain RGBA8 textures now
   get the fill uploaded instead (`engine2/src/render/texture.rs`).
2. **Fixed:** the GPU spun forever in `sprite/sprite_cs`. See [Rendering](#rendering).

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

What's left, and why:

- No textures, no sky texture: the zoo's materials are `reflectivity_30.vmat` and
  `skybox_day_01.vmat`, whose `.tga`/`.exr` aren't in the repo and no `.vtex_c` ships. The sky is
  our flat placeholder.
- The one spotlight is the `studio_light_projector` prop's; the scene itself has only the sun.
- No HUD visible yet; not triaged.
- `downsample_spd` skips (Rg32Float mip through an Rgba32Float view), and CMAA2's indirect
  passes are no-ops, so it never blends (docs/renderer.md, Next).
- Grubs: `gr_foreground*` placeholder shaders fail GPU validation and `new_water.shader` fails
  to compile; not triaged.

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

## GameResources and snapshots

The 39 `.geq_c/.sdflayer_c/.surface_c` files are `RERL` (only when the resource has references),
`REDI`, then `DATA`. `DATA` holds the source JSON plus a NUL. Each file's JSON equals its source,
so the "stored as authored" assumption holds for these types. `.vsnap_c` is `REDI`, `DATA` (binary
KV3, which `kv3bin` decodes) and a `SNAP` block. Check them with
`UNDERBOX_REAL_RESOURCES=<dirs> cargo test -p assets --lib real_resource -- --nocapture`.

## Unconfirmed guesses

- Feature record min: assumed to be the i32 before max. Every real record has min 0.
- The two u32s between a combo's `-3` and its zstd frame, assumed to be sizes. Unchecked, because
  frames can't be decoded.
- The `GUID` block's meaning, and what makes some files resource version 2, 3 or 4.
- `DBLOB` as a block name: no real file has one.

## Fixed root causes

- **Physics:** shapes made with `DefaultSolidTag` lost "solid" when their GameObject had tags.
  `WithCollisionRules` lists only tags the rules name, so every PlayerController ground trace
  missed the zoo's `world`-tagged floor. The fix is in `ShapeMeta::base_tag`.
- **Shaders:** native-masked embedded HLSL didn't parse. It now does, as described in
  [Shaders](#shaders).
- **Render:** `Texture.Clear` on plain textures, as described in [Client frame](#client-frame);
  the `sprite_cs` hang and the grey frame, as described in [Rendering](#rendering).
