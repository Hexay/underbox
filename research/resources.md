# Compiled resources (`.vmdl_c`, `.vtex_c`, `.vmat_c`, `.vphys_c`, `.vagrp_c`/`.vanim_c`, `.vmorf_c`, `.vsnd_c`)

Readers and writers: `crates/assets/src/compiled/`. Engine glue: `crates/engine2/src/model/compiled.rs`,
`model/physics/compiled.rs`, `compiled/{texture,material,anim}.rs`, `audio/compiled.rs`. Container framing: see
research/resource-container.md.

Sources (format reference only, nothing copied or downloaded from Facepunch/Steam):
- ValveResourceFormat (MIT) @624fda5: `ValveResourceFormat/Resource/ResourceTypes/{Model,Mesh,Texture,Material,PhysAggregateData,Morph,Sound}.cs`,
  `Blocks/VBIB.cs`, `Utils/MeshOptimizerVertexDecoder.cs`, `MeshOptimizerIndexDecoder.cs`, `Texture/TextureExtraData.cs`, `VTexFormat.cs`.
- VRF animation: `ResourceTypes/ModelAnimation/{Animation,AnimationDataChannel,AnimationSegmentDecoderContext,FrameBlock}.cs`,
  `ModelAnimation/SegmentDecoders/*.cs` (the `CCompressed*` decoders), `ModelAnimation/Sequence*.cs`.
- VRF rig and flex: `ResourceTypes/ModelData/{Attachments/Attachment,Hitbox,HitboxSet}.cs`, `ModelFlex/{FlexController,FlexRule}.cs`,
  `ModelFlex/FlexOps/*.cs`, `Enums/FlexOpCode.cs`.
- VRF physics: `ResourceTypes/RubikonPhysics/{Joint,Shape,ShapeDescriptor,Shapes/Compound}.cs`; transforms per
  `Serialization/KeyValues/KVObjectExtensions.cs` `ToTransform`.
- meshoptimizer (MIT) @f0a684f: `src/vertexcodec.cpp`, `src/indexcodec.cpp`; reference streams from `demo/tests.cpp`.
- bcdec_rs 0.2 (MIT): BC1-BC7 block decode, BC6H to half floats.

Fixtures are synthetic: `assets::compiled::fixture` writes every file (the crate model, the rigged model with its
atlas, animation group and `.vanim`, a sound); `world::fixture::tiny_map` packs the crate into corpus/mapworld's
`maps/tiny.vpk`.

## Models (`.vmdl_c`)

| Block | Holds |
|---|---|
| DATA | `m_name`, `m_modelSkeleton` (parallel bone arrays), `m_refMeshes` (empty string = embedded), `m_refMeshGroupMasks`, `m_refLODGroupMasks`, `m_meshGroups`, `m_nDefaultMeshGroupMask`, `m_lodGroupSwitchDistances`, `m_materialGroups`, `m_refPhysicsData`, `m_refAnimGroups`, `m_animGraph2Refs[].m_hGraph`, `m_modelInfo.m_keyValueText` |
| CTRL | old: `embedded_meshes[] {name, mesh_index, data_block, vbib_block, morph_block}`; new: `{m_Name, m_nMeshIndex, m_nDataBlock, m_nMorphBlock, m_vertexBuffers/m_indexBuffers[].m_nBlockIndex}` (MVTX/MIDX); `embedded_physics.phys_data_block`; `embedded_animation {group_data_block, anim_data_block, seqgroup_data_block}` |
| MDAT | mesh DATA: `m_sceneObjects[].m_drawCalls[]`, bounds, `m_attachments`, `m_hitboxsets`, `m_morphSet` |
| MBUF/VBIB | binary vertex/index buffers |
| MRPH | morph set, as `.vmorf_c` DATA |
| PHYS | aggregate, as `.vphys_c` DATA |
| AGRP / ANIM / ASEQ | animation group (`m_decodeKey`), its animations, its sequences |

Skin N maps material `group0[i]` to `groupN[i]`.

**Key values:** `m_keyValueText` is KV3 text. Its tree is the model's game data (`FindModelSubKey`, `Model.GetData<T>`),
and `anim_graph_resource` names the graph (else `m_animGraph2Refs[0]`).

**Geometry-free read:** `model::read_rig` skips vertex buffers and morph atlases; `model::bones` decodes only DATA.

**Binary VBIB:** header `{vbOffset (from field 0), vbCount, ibOffset (from field 8), ibCount}`. The 24-byte record is
`{count, sizeFlags, attrOffset (from +8), attrCount, dataOffset (from +16), dataSize}`. The flags:
- size = `sizeFlags & 0x3FFFFFF`;
- bit 0x4000000 **clear** = meshopt;
- 0x8000000 = zstd.

Attributes are 56 bytes: `char[32]` name, then SemanticIndex, Format (DXGI), Offset, Slot, SlotType, StepRate.

**Gotchas:**
- Data at least `count*stride` long is raw whatever the flags say. Otherwise it is zstd first, then meshopt.
- The zstd frame size is unknown, so stream-decode it (`decode_all_to_vec` needs preallocated capacity).
- The meshopt index codec keeps winding, not the first vertex: decoded triangles can be rotated.
- Each draw call's `m_nBaseVertex` is folded into the indices at load. The renderer ignores base vertex.

**Compressed normals:**
- `R8G8B8A8_UNORM` NORMAL is v1: 2 bytes per normal or tangent, with tangent w = `b3 < 128 ? -1 : 1`.
- `R32_UINT` is v2: sign, 11-bit tangent angle, then 10+10 bit octahedral.

## Animation (ANIM, `.vagrp_c`, `.vanim_c`)

**Group:** `.vagrp_c` DATA has `m_decodeKey` and either an ANIM block or `m_localHAnimArray` (`.vanim` paths, each DATA an
`AnimationResourceData_t`). A model's own group is AGRP + ANIM.

**`AnimationResourceData_t`:**
- `m_decoderArray[].m_szName` names each segment's decoder; `m_segmentArray[]` holds `{m_container, m_nLocalChannel}`.
- `m_animArray[]`: `m_name`, `fps`, `m_flags {m_bLooping, m_bDelta, m_bHidden, m_bAnimGraphAdditive}`, and
  `m_pData {m_nFrames, m_frameblockArray[]}` (older files keep `numframes`/`m_frameblockArray` on the desc).
- Frame blocks: `{m_nStartFrame, m_nEndFrame (inclusive), m_segmentIndexArray}`.

**Segment container:** i16 decoder, cardinality, element count, total length; the element ids (i16); then data.
Keyed decoders are frame-major (`frame * count + element`), frames relative to the block.

**Decode key:** `m_dataChannelArray[] {m_szChannelClass (BoneChannel/MorphChannel), m_szVariableName (Position/Angle/Scale/data),
m_szElementNameArray, m_nElementIndexArray}`. Elements bind to bones or flex controllers **by name**.

**Decoders:** 48-bit quaternion (three 14-bit components with a half-select bit; words 0–1's sign bits pick the dropped
component, word 2's is its sign), half3 positions, base + half
delta, full floats/vectors, static variants. Unkeyed bones hold the bind pose (identity in a delta).

**Sequences (ASEQ):** `m_localSequenceNameArray`, `m_localS1SeqDescArray[] {m_sName, m_flags, m_fetch {m_localReferenceArray,
m_nGroupSize, m_nLocalPose, m_poseKeyArray0/1}}`, `m_localPoseParamArray[] {m_sName, m_flStart, m_flEnd}`. Blends bake at
each pose parameter's 0 (nothing sets them; see `crates/assets/src/compiled/anim/blend.rs`). Field names past
`m_localReferenceArray` follow VRF and are unchecked against a shipped model.

**Events:** an animation desc's `m_eventArray[] {m_nFrame, m_flCycle, m_sEventName, m_EventData}`; managed reads the payload
in `SceneModel.OnAnimationEvent`.

## Morphs (MRPH, `.vmorf_c`)

**`MorphSetData_t`:** `m_nWidth`/`m_nHeight` (the vertex grid), `m_bundleTypes` (position/speed, normal/wrinkle),
`m_morphDatas[] {m_name, m_morphRectDatas[]}`, `m_FlexDesc[].m_szFacs`, `m_FlexControllers`, `m_FlexRules`, `m_pTextureAtlas`.

**Rects:** `{m_nXLeftDst, m_nYTopDst, m_flUWidthSrc, m_flVHeightSrc, m_bundleDatas[] {m_flULeftSrc, m_flVTopSrc, m_ranges, m_offsets}}`.
A texel decodes as `rgba / 255 * range + offset`. Grid cell `y * width + x` is that mesh vertex.

**Flex rules:** stack programs (`FLEX_OP_*`, by number or name) from controller values to descriptor weights. Op data
that names a controller is an int; an index popped off the stack is the float's bit pattern. 2-way/n-way/eyelid ops
read controllers remapped from their own min/max (see `crates/assets/src/compiled/flex/eval.rs`).

## Attachments and hitboxes (mesh data)

- `m_attachments[]` (or `{key, value}` pairs): `m_name`, `m_bIgnoreRotation`, `m_nInfluences`, `m_influenceNames`,
  `m_vInfluenceOffsets`, `m_vInfluenceRotations`, `m_influenceWeights`, `m_bInfluenceRootTransform`.
- `m_hitboxsets[]` (or pairs): `m_name`, `m_HitBoxes` or `m_hitboxes[] {m_name, m_sSurfaceProperty, m_sBoneName, m_vMinBounds,
  m_vMaxBounds, m_flShapeRadius, m_nShapeType (0 box, 1 sphere, 2 capsule), m_bTranslationOnly, m_nGroupId}`.
- Every mesh may carry them; the first of each name wins.

## Textures (`.vtex_c`)

**DATA layout:**
- A 40-byte header (v1) holds flags, reflectivity, w/h/depth, format, mips and the extra-data table (offsets count from their own field).
- Pixels run from the end of the DATA block to EOF, mips smallest first. Each mip holds depth × (6 if CUBE) surfaces.

**Gotchas:**
- Block formats pad to 4x4, and VRF counts 2–3 layers as 4.
- `COMPRESSED_MIP_SIZE` (type 4) with method 1 means each mip is a raw LZ4 block. A mip stored at full size is raw.
- There is no sRGB flag. Normal-map and YCoCg handling comes from REDI special deps (not read).

**Decoding:** DXT1/5, BC4/5/6H/7, the 8/16/32-bit uncompressed formats and PNG/JPEG decode. ETC2/EAC, WebP and R32_UINT don't.

## Materials (`.vmat_c`)

**DATA fields:**
- `m_materialName` and `m_shaderName`.
- The param lists: `m_intParams` (`m_nValue`, including `F_*` static combos), `m_floatParams` (`m_flValue`), `m_vectorParams` (`m_value`) and `m_textureParams` (`m_pValue`).
- The `m_*Attributes` lists.

**Not read:** dynamic (VfxEval) params and INSG.

## Physics (`.vphys_c`, PHYS)

**Top level:** `m_parts`, `m_bindPose[]` (3×4 rows), `m_boneNames`, `m_boneParents` (part → bone), `m_surfacePropertyHashes` (StringToken of the surface name), `m_collisionAttributes` (`m_InteractAsStrings`, or legacy `m_PhysicsTagStrings`) and `m_joints`.

**Parts:** each `m_rnShape` has `m_spheres`, `m_capsules`, `m_hulls`, `m_meshes` and `m_compounds`. Every descriptor carries `m_nSurfacePropertyIndex`/`m_nCollisionAttributeIndex`.

**Compounds:** `m_Compound {m_Spheres, m_Capsules, m_Hulls, m_Meshes}` hold bare shapes that take the descriptor's ids; they're
flattened into the part. `m_CompoundMaterialIndices` isn't read.

**Hull vertices:** `m_VertexPositions` (f32x3 blob) when present, else `m_Vertices`.

**Meshes:** `m_Triangles` is an i32x3 blob or `{m_nIndex[3]}` objects. `m_Materials` is each triangle's index into the
surface list; traces report the triangle and its surface. Hull and mesh BVH/topology are skipped; Rapier rebuilds them.

**Joints:** `m_nType` (1 spherical, 2 prismatic, 3 revolute, 4 conical, 6 weld, 16 wheel), `m_nBody1/2` (parts), `m_nFlags`
(1 = body 1 fixed), `m_Frame1/2` (pos, scale, quat or pos, quat), `m_bEnable{Collision,LinearLimit,SwingLimit,TwistLimit,
LinearMotor,AngularMotor}`, the `{m_flMin, m_flMax}` limits (radians), motor targets and maxima, spring frequency/damping, `m_flFriction`.

Map collision is `maps/<name>/world_physics.vphys`, reached in two ways:
- `Glue.Resources.GetPhysics` → MapInstance → MapCollider.
- `IPhysicsWorld::CreateAggregateInstance(name)` from Map.cs.

## Sounds (`.vsnd_c`)

- **v1–v3 DATA:** a bit-packed u32 (type bits 0–1, sample bits 2–6, channels 7–8, wave format 12–13, rate 14–29), then loop start,
  sample count, duration, sentence offset, `m_pHeader` (offset from its field, size), streaming size; v1 adds an unused
  array, v2 a morph pointer.
- **v4 DATA:** u16 rate, u8 format (0 PCM16, 1 PCM8, 2 MP3, 3 ADPCM), u8 channels, the same fields, then loop end.
- **v5:** empty DATA. CTRL `{_class CVoiceContainerDefault/Envelope, m_vSound {m_nFormat, m_nRate, m_nChannels,
  m_nSampleCount, m_flDuration, m_nLoopStart, m_nLoopEnd, m_nStreamingSize}}`. A KV3 DATA with those fields also reads.
- **Stream:** trails the container at its header's file size. PCM and ADPCM are bare samples (ADPCM's `fmt ` chunk is the
  v1–4 header), wrapped in RIFF to decode; MP3 is a whole file.
- Managed never reads vsnd bytes: `PrecacheSound("x.vsnd")` resolves a source file first, then `x.vsnd_c`.
