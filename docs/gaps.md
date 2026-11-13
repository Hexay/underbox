# Gaps

What underbox doesn't port yet. Every `igen_engine` slot is bound (`cargo run -p engine2 --bin
coverage` reports no stubs), so a gap is a slot bound to a no-op or an approximation. Most rows
also carry a one-line `TODO` at their site; `rg -n TODO crates --glob '!crates/abi/**'` lists them.
Keep this table and those comments in step.

Size: **S** up to ~50 lines, **M** a few hundred, **L** a new subsystem or dependency.
**verify**: works, but a guessed value needs checking against real engine2 (docs/differential.md).

| Area | Missing | Where | Real engine2 | Size |
| --- | --- | --- | --- | --- |
| Video | Decoding and encoding: a player never loads, a recorder never takes frames | `crates/engine2/src/video.rs:4` | Decodes mp4/webm into a texture | L, in progress |
| Skinning | Morph and deformation volumes in `DispatchModelMeshSkinning` | `crates/engine2/src/render/skin/dispatch.rs:87` | Applied on the GPU (docs/skinning.md) | M, in progress |
| VR | `RenderStereo`/`SubmitStereo`/`BlitStereo` are no-ops | `crates/engine2/src/render/scene/camera.rs:258` | OpenXR through openxr_loader | L |
| HTML surface | Always null | docs/steam.md "HTML surface" | Steam's HTML surface | L |
| Render | `RenderToCubeTexture` draws nothing | `crates/engine2/src/render/scene/camera.rs:258` | Six views into the cube's faces | M |
| Render | Clears ignore a viewport or scissor smaller than the target | `crates/engine2/src/render/context.rs:93` | Clears only the rect | M |
| Render | `CopyTexture` copies mip 0, layer 0 only | `crates/engine2/src/render/tools.rs:33` | Any mip and slice | S |
| Render | `GenerateMipMaps`, `ClearTextureUint` are no-ops | `crates/engine2/src/render/tools.rs:190` | Downsample chain; integer clear | M |
| Render | `DrawInstancedIndirect`, `DrawIndexedInstancedIndirect` are no-ops | `crates/engine2/src/render/draw/immediate.rs:256` | Args read on the GPU | M |
| Render | `DrawModel_1/3/4` ignore GPU-buffer transforms and args | `crates/engine2/src/render/draw/immediate.rs:274` | Transforms from the buffer | M |
| Render | Volume textures have no GPU side | `crates/engine2/src/render/texture.rs:128` | 3D textures | M |
| Render | GPU sprite sheets: `GetSheetMetaDataTexture` null, `GetSheetInfo` zero, shader sees one full frame, decals unsheeted | `crates/shaders/hlsl/builtin/sheet_sampling.fxc:2`, `crates/engine2/src/render/lighting/decals.rs:91` | Sheet sequences sampled per frame | M |
| Render | Decal normal maps and parallax | `crates/engine2/src/render/shading/standin_decals.hlsl:4` | Applied | M |
| Render | Baked light probe volumes and lightmaps | `crates/shaders/hlsl/builtin/light_probe_volume.fxc:3` | Baked indirect light (docs/lighting.md) | L |
| Render | Per-light diffuse/specular/transmissive switches, bake indices | `crates/engine2/src/scene/light.rs:169` | Honoured | M |
| Render | Culling boxes are ids only | `crates/engine2/src/render/scene/system.rs:64` | `SceneCullingBox` hides objects inside or outside | S |
| Render | Scene object material overrides by attribute or index, material groups, body groups | `crates/engine2/src/scene/object_slots.rs:98` | Applied | M |
| Render | `TextureDim`/`InvTextureDim` shader globals | `crates/engine2/src/render/shading/view.rs:64` | The paired texture's size | S |
| Render | Gizmo free-form (lattice) deformation | `crates/shaders/hlsl/builtin/ffd.fxc:2` | Deforms | M |
| Render | A double sRGB encode or decode in one blit | `crates/engine2/src/render/draw/mod.rs:131` | Applied (no pipeline asks) | S |
| Render | Window surfaces without `COPY_DST` | `crates/render/src/surface.rs:65` | Presents | S |
| Render | Closed core shaders (complex, generic, unlit, sky, ...) draw with stand-ins | docs/renderer.md, `crates/engine2/src/render/shading/standin.rs` | Its own shaders | L |
| Render | Per-frame stats beyond views, draw calls and triangles; GPU timestamps and frame time | `crates/engine2/src/render/stats.rs` | Counted | M |
| Render | BC1 three-colour blocks sample alpha 0 at index 3 | `crates/engine2/src/compiled/texture.rs:61` | Alpha 1 | S, deliberate |
| Render | RGBM range 6 | `crates/shaders/hlsl/builtin/math_general.fxc:47` | Unknown | verify |
| Render | Ortho light type value 4 is ours | `crates/engine2/src/scene/light.rs:22` | Unknown | verify |
| Animation | Graph float channels, external graph slots | `crates/engine2/src/anim/controller.rs:123` | From the graph's data | M |
| Animation | Bounds are the model's render bounds | `crates/engine2/src/anim/bone_slots.rs:99` | Posed bounds (the citizen's reach 3.75 below) | S |
| Animation | FBX/DMX import: skeletons and clips come only from runtime builders | `crates/anim/src/source/vmdl.rs:3` | ModelDoc compiles them | L |
| Animation | AnimGraph1 node graphs: parameters and tag spans only | `crates/anim/src/source/vanmgrph.rs:4` | Evaluated | L |
| Animation | AnimGraph2 node classes without support compile to neutral stand-ins (logged once per graph) | `crates/anim/src/source/ag/mod.rs:4` | Evaluated | M each |
| Physics | Hull simplification (precision, face/edge/vertex caps) | `crates/engine2/src/model/physics/desc.rs:124` | Simplified hull | M |
| Physics | Wheel spin and steering torques both report the total angular impulse | `crates/engine2/src/physics/joint_drive_slots.rs:67` | Per axis | S |
| Resources | Hot reload | `crates/engine2/src/resource_system.rs:37` | Recompiles and swaps | M |
| Resources | Unreferenced resources stay resident | `crates/engine2/src/resource.rs:50` | Unloaded | M |
| Resources | Game-defined GameResource types aren't compiled | `crates/assets/src/game_resource.rs:14` | Compiled | M |
| Resources | GameResource JSON stored as authored | `crates/assets/src/game_resource.rs:20` | `ScanJson` rewrites paths and embedded resources | M |
| Resources | Saved model blocks are KV3 text | `crates/engine2/src/mesh/serialize.rs:3` | Binary block layouts | L |
| Resources | KV3 saves as text | `crates/engine2/src/kv3/mod.rs:112` | `KV3_ENCODING_BINARY_AUTO_COMPRESSED` | M |
| Resources | Reference-list id hash | `crates/assets/src/compiled/mod.rs:115` | Unknown | verify |
| Mesh | UV generation is a planar projection per face | `crates/engine2/src/mesh/tools.rs:38` | LSCM unwrap | M |
| Mesh | LZ4 HC levels compress as plain LZ4 | `crates/engine2/src/compress.rs:2` | HC encoder | S |
| Audio | Lip-sync vowels from formants | `crates/audio/src/lipsync.rs:3` | OVRLipSync | M |
| Audio | Reverb low band; Steam Audio's parametric reverb | `crates/audio/src/effects/reverb.rs:3` | Steam Audio | M |
| Audio | DSP delay-time modulation, delay types beyond plain and low-passed | `crates/audio/src/dsp/units.rs:2` | Supported | M |
| Window | macOS native surface | `crates/window/src/handle.rs:11` | n/a (no macOS build) | M |
| Window | Windows OLE drag and drop | `crates/window/src/ext.rs:28`, `:34` | Supported (docs/windowing.md) | M |
| Window | Windows chrome: DWM, extended styles, taskbar | `crates/window/src/ext.rs:40` | Supported | S |
| App | Resident memory on Windows | `crates/engine2/src/app/diagnostics.rs:8` | `GetProcessMemoryInfo` | S |
| App | Native ConCommands: the plumbing exists, none are defined | `crates/engine2/src/concommand.rs:3` | Engine commands | S each |
| Platform | Game server runs without VAC (`bSecure` false) | `crates/platform/src/steam.rs:69` | Secure mode | S, deliberate |
| Platform | Steam services left null or empty | docs/steam.md | Real | per row |
