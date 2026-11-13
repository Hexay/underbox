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
  (`skin/cache.rs`, 24 bytes per vertex), as real does: real skins in compute, never in the
  vertex stage. Its input is the mesh's own draw stream, real's 68-byte float layout
  (`Layout::skinned_draw_vertex`, `gpu_mesh::draw_layout`).
- Cache draws set `D_CS_VERTEX_ANIMATION` (`skin::VERTEX_ANIMATION`). That variant of the stand-in
  and of the depth stand-in reads the cache at `nVertexCacheOffset + nVertexCacheIndex`, as
  `vr_common_vs_code.fxc` does. `nVertexIndex`/`nVertexCacheIndex` (`TEXCOORD14`/`15`) come from a
  shared 0, 1, 2... float stream in real's vertex slots 2 and 3 (`shading/vertex.rs`). Shadow and
  prepass draws keep the draw's own transform entry and switch to the depth variant
  (`caster::on_depth`): a caster instance's entry has no cache offset, which is what drew the
  prepass in rest pose before.
- Morphs composite on the GPU as real's do: one render pass per batch into a 2048×1024
  `R16G16B16A16Sfloat` atlas (`skin/morphs.rs`, `render/src/morph.rs`), cleared by the frame's
  first. Each target × region is one additive quad, zero-weight targets included; position deltas
  go in the left half and normal deltas at the same texel in the right. Regions are shelf-packed
  from texel (1, 1), a texel apart. Our `morph.fxc` `Morph()` loads the atlas at the subrect named
  in the morph entry (u, width, v, height in atlas uv).
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
  - `anim/flex.rs` samples morphs at the player's `sample_time` (which honours
    `SetSequenceLooping`) by frame, so a looping clip played with `Looping=false` settles.
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
pixels). Bound-buffer and `RenderTools.Draw` gizmo lines take the same path, via read-back
(`gizmo::expand_bound`).

## Render systems we don't have: what managed is told

| Slot(s) | Answer | Managed's path |
| --- | --- | --- |
| `IRayTraceSceneWorld` build, `IsRayTracingSupported` | no-op, `false` | skips ray-traced effects |
| `g_pSceneUtils` tonemap | auto exposure measured and applied as `g_flToneMapScalarLinear` (render/exposure.rs) | managed's tonemapping.shader applies the curve |
| `g_pSceneUtils` volumetric fog | froxels from `AddVolumetricFogVolume`, sampled through `volumetric_fog.fxc` | `SetParams` is called unguarded, so null isn't an option; see docs/fog.md |
| `GetBestMultisampleType`, `IsUpscalerSupported`, `SupportsAsyncCompute` | 0 (none), `false`, `false` | single-sample targets, no upscaler, no async queue |
| Texture web streaming | inactive, no requests, prefetch done | loads stay local |
| Sprite sheets (`SheetSequence*`) | no frames | static textures |
| `CalculateRadianceSH`, probe constants | no-op | probes stay empty (docs/lighting.md) |
| 3D skybox, PVS | stored by the world lane; drawn as a child view (render/scene/skybox.rs) and culled per object (render/scene/pvs.rs) | |
| Mesh traces | the world lane's (`crates/engine2/src/world`) | |

## Sampler states

`FindOrCreateSamplerState` registers each distinct `CSamplerStateDesc` once (`render/samplers.rs`).
Its index is the bindless sampler index. Draws get the wgpu sampler the description defines, both
in the `g_bindless_Sampler*` arrays and for sampler attributes set by name:

- filter uses D3D11 bits: 0x01 linear mips, 0x04 linear magnification, 0x10 linear minification,
  0x40 anisotropic, with `2^AnisoExp` as the anisotropy;
- address modes map Wrap → Repeat, Mirror and MirrorOnce → MirrorRepeat, Clamp → ClampToEdge and
  Border → ClampToBorder (transparent black);
- non-filtering bindings force nearest filtering, and comparison bindings keep the defaults.
