# Compiled resources (`.vmdl_c`, `.vtex_c`, `.vmat_c`, `.vphys_c`)

Readers and writers: `crates/assets/src/compiled/`. Engine glue: `crates/engine2/src/model/compiled.rs`,
`model/physics/compiled.rs`, `compiled/{texture,material}.rs`. Container framing: see
research/resource-container.md.

Sources (format reference only, nothing copied or downloaded from Facepunch/Steam):
- ValveResourceFormat (MIT) @624fda5: `ValveResourceFormat/Resource/ResourceTypes/{Model,Mesh,Texture,Material,PhysAggregateData}.cs`,
  `Blocks/VBIB.cs`, `Utils/MeshOptimizerVertexDecoder.cs`, `MeshOptimizerIndexDecoder.cs`, `Texture/TextureExtraData.cs`, `VTexFormat.cs`.
- meshoptimizer (MIT) @f0a684f: `src/vertexcodec.cpp`, `src/indexcodec.cpp`; reference streams from `demo/tests.cpp`.
- bcdec_rs 0.2 (MIT): BC1-BC7 block decode, BC6H to half floats.

Fixtures are synthetic: `assets::compiled::fixture` writes every file; `world::fixture::tiny_map` packs them
into corpus/mapworld's `maps/tiny.vpk`.

## Models (`.vmdl_c`)

| Block | Holds |
|---|---|
| DATA | `m_name`, `m_modelSkeleton` (parallel bone arrays), `m_refMeshes` (empty string = embedded), `m_refMeshGroupMasks`, `m_refLODGroupMasks`, `m_meshGroups`, `m_nDefaultMeshGroupMask`, `m_lodGroupSwitchDistances`, `m_materialGroups`, `m_refPhysicsData`, `m_modelInfo.m_keyValueText` |
| CTRL | old: `embedded_meshes[] {name, mesh_index, data_block, vbib_block}`; new: `{m_Name, m_nMeshIndex, m_nDataBlock, m_vertexBuffers/m_indexBuffers[].m_nBlockIndex}` (MVTX/MIDX); `embedded_physics.phys_data_block` |
| MDAT | mesh DATA: `m_sceneObjects[].m_drawCalls[]`, bounds |
| MBUF/VBIB | binary vertex/index buffers |
| PHYS | aggregate, as `.vphys_c` DATA |

Skin N maps material `group0[i]` to `groupN[i]`.

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

**Top level:** `m_parts`, `m_bindPose[]` (3×4 rows), `m_boneNames`, `m_boneParents` (part → bone), `m_surfacePropertyHashes` (StringToken of the surface name) and `m_collisionAttributes` (`m_InteractAsStrings`, or legacy `m_PhysicsTagStrings`).

**Parts:** each `m_rnShape` has `m_spheres`, `m_capsules`, `m_hulls` and `m_meshes`. Every descriptor carries `m_nSurfacePropertyIndex`/`m_nCollisionAttributeIndex`.

**Hull vertices:** `m_VertexPositions` (f32x3 blob) when present, else `m_Vertices`.

**Meshes:** `m_Triangles` is an i32x3 blob or `{m_nIndex[3]}` objects. Hull and mesh BVH/topology are skipped; Rapier rebuilds them.

**Not read:** joints (`m_joints`) and compounds.

Map collision is `maps/<name>/world_physics.vphys`, reached in two ways:
- `Glue.Resources.GetPhysics` → MapInstance → MapCollider.
- `IPhysicsWorld::CreateAggregateInstance(name)` from Map.cs.
