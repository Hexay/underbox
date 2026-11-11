# Skinning, morphs, gizmos and render systems we don't have

Part of the renderer (docs/renderer.md). Code: `crates/engine2/src/render/skin/`,
`render/draw/{dynamic,gizmo,screen_rect}.rs`, `render/shading/standin_gizmo_*.shader`,
`render/scene_utils.rs`, `render/samplers.rs`.

## Skinning and morphs (both render paths)

Native's model, which managed's `MeshRenderFeature.Skinning.cs` mirrors:

- An animated object writes three things into a transform buffer (`transform_buffer.fxc`, 64-byte entries):
  - a base entry: `Row0 = (0,0,1,1)`, the vertex cache offset and the blend weight count;
  - a morph entry at slot + 1, holding the morph subrect;
  - one entry per bone at slot + 2 + b, each world pose × inverse bind pose, stored as rows.
- Meshes with one blend weight and no morphs skin in the vertex shader (`GetTransformMatrix(slot,
  bone)`). Everything else runs `skinning_cs` (translated from sbox-public) into the vertex cache
  (`skin/cache.rs`, 24 bytes per vertex). The stand-in then draws from the cache whenever the base
  entry has a cache offset and blend weights.
- Morphs are summed on the CPU per object per frame into one composite buffer (`skin/morphs.rs`).
  Our `morph.fxc` `Morph()` reads that buffer at the subrect named in the morph entry.
- Managed drives all of this through `RenderTools` (`GetModelMeshSkinning*`,
  `AllocateVertexCache`, `DispatchModelMeshSkinning`, `QueueModelMeshMorph`, `GenerateMorphs`).
  Engine-drawn objects go through `skin/native.rs`.
- A mesh is skinned when its model has more than one bone and its layout carries 8-bit bone
  indices (`Layout::sbox_skinned_vertex`).
- Gotchas:
  - Storage buffers bind by HLSL name or by vfx `Attribute("...")` name (`shading/bind.rs`).
    skinning_cs's `InputVB`/`OutputVB` are attribute names, and missing them gave all-zero output.
  - Managed's `Matrix3x4[]` is only 4-byte aligned. Write it as `[[f32; 4]; 3]`, never as glam's
    `Vec4`.
  - A model with a ragdoll engages `BonePhysics`, which holds the bones in bind pose. The
    `corpus/skinned` puppet fixture (`crates/assets/src/compiled/fixture/puppet.rs`) therefore has
    no physics, and its `bow` clip doesn't loop, so the frame settles.
  - The anim lane's `flex.rs` samples morphs at raw `sequence.time` with the clip's own looping
    flag. A looping clip's morphs therefore never settle, even with `Looping=false` (not ours to fix).
- Repros: `sh tools/test.sh engine2 skin::tests` (the fixture, skinning_cs output, two-weight
  blending) and `CORPUS=skinned [MANAGED=1] sh tools/client-frame.sh`.

## Gizmo geometry shaders

naga can't translate `MainGs`, so gizmo_line and gizmo_sprite are emulated:

1. A CPU pre-pass (`draw/gizmo.rs`) expands each line segment or point of a dynamic object whose
   material maps to a gizmo stand-in into a quad of corner vertices, two triangles each. Every
   corner carries what the GS read from the primitive's other vertices:
   - lines: `tex1` holds the other endpoint and the side to extrude to, and `tangent` holds the
     first endpoint (for dashes);
   - sprites: `tex1.xy` holds the corner.
2. The stand-ins (`standin_gizmo_line.shader`, `standin_gizmo_sprite.shader`) run the GS's
   screen-space maths in the vertex shader: the near-plane clip, `LineThickness` pixels of
   extrusion and depth bias for lines; screen- or world-space size and rotation for sprites. They
   keep the original pixel shaders and render states.

The camera's forward and up vectors (`g_vCameraDirWs`, `g_vCameraUpDirWs`) come from the
view-projection. GPU repro: `sh tools/test.sh engine2 gizmo_tests` (line width and sprite size in
pixels). Lines in GPU buffers (none in sbox-public's gizmo or DebugOverlay code) still draw 1 px.

## Render systems we don't have: what managed is told

| Slot(s) | Answer | Managed's path |
| --- | --- | --- |
| `IRayTraceSceneWorld` build, `IsRayTracingSupported` | no-op, `false` | skips ray-traced effects |
| `g_pSceneUtils` tonemap / volumetric fog | real objects that store their parameters; not applied | `SetParams` is called unguarded, so null isn't an option |
| `GetBestMultisampleType`, `IsUpscalerSupported`, `SupportsAsyncCompute` | 0 (none), `false`, `false` | single-sample targets, no upscaler, no async queue |
| Texture web streaming | inactive, no requests, prefetch done | loads stay local |
| Sprite sheets (`SheetSequence*`) | no frames | static textures |
| `CalculateRadianceSH`, probe constants | no-op | probes stay empty (docs/lighting.md) |
| 3D skybox, PVS, mesh traces | the world lane's (`crates/engine2/src/world`) | |

## Sampler states

`FindOrCreateSamplerState` registers each distinct `CSamplerStateDesc` once (`render/samplers.rs`).
Its index is the bindless sampler index. Draws get the wgpu sampler the description defines, both
in the `g_bindless_Sampler*` arrays and for sampler attributes set by name:

- filter uses D3D11 bits: 0x01 linear mips, 0x04 linear magnification, 0x10 linear minification,
  0x40 anisotropic, with `2^AnisoExp` as the anisotropy;
- address modes map Wrap → Repeat, Mirror and MirrorOnce → MirrorRepeat, Clamp → ClampToEdge and
  Border → ClampToBorder (transparent black);
- non-filtering bindings force nearest filtering, and comparison bindings keep the defaults.
