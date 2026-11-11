# Compiled maps: VPK, world, world nodes, entity lumps

Sources: ValveResourceFormat (VRF, MIT) read as code, not run: `Resource/ResourceTypes/BinaryKV3*.cs`,
`World.cs`, `WorldNode.cs`, `EntityLump.cs`, `Enums/EntityFieldType.cs`, `Utils/StringToken.cs`,
`Renderer/World/WorldNodeLoader.cs`, `Serialization/KeyValues/KVObjectExtensions.cs` (`ToMatrix4x4`). VPK
layout from Valve Developer Community knowledge (VRF uses the separate ValvePak package). Managed callers from
sbox-public: `Systems/SceneSystem/SceneMap.cs`, `Game/Map/MapLoader.cs`, `Scene/Components/Map/MapInstance.cs`,
`Definitions/common/Render/IWorldRendererMgr.def`.

**Nothing here is checked against a real Facepunch map**: no compiled map ships in sbox-public and we don't
download any. Every reader is tested against our own writers (`world::fixture`, `assets::kv3bin::write`).

## What managed asks of native

- `SceneMap` calls `CreateWorld("maps/foo.vpk", sceneWorld, ...)`, then `GetFolder()` (`maps/foo`),
  `PrecacheAllWorldNodes`, `g_pEnginePVSManager.BuildPvs`, `World.AddSceneMap`, then
  `MapLoader.CreateEntities(worldRef, "default_ents")`: `GetEntityCount(lump)` and per index
  `GetEntityKeyValues` → `CEntityKeyValues.GetKeyCount/GetKey(i)` (a `StringToken` hash) /
  `GetValueString(hash, null)`. Values are text; managed parses with `ToType` (vectors `x y z`, bools 1/0).
- `MapInstance` then loads `{folder}/world_physics.vphys` and `{folder}/world.scene_c` itself (warns if
  missing). `SceneMap.CreateAsync` polls `IsWorldLoaded` with `ServiceWorldRequests`; `Delete` calls
  `Release` then `ServiceWorldRequests` to tear down while the scene world lives.
- `SceneNetworkSystem` calls `MountWorldVPK(stem, "maps/x.vpk")` for package maps.

## VPK (v1, v2)

Header: `u32 0x55AA1234, u32 version, u32 treeSize`; v2 adds `fileDataSize, archiveMd5Size, otherMd5Size,
signatureSize` (28 bytes). Tree: `ext\0 { dir\0 { file\0 entry }* \0 }* \0 ... \0`, `" "` for no dir/ext.
Entry: `u32 crc32, u16 preloadBytes, u16 archiveIndex, u32 offset, u32 length, u16 0xFFFF`, preload bytes.
Index 0x7FFF: data in the dir file at `headerSize + treeSize + offset`; otherwise `<name>_NNN.vpk` beside
`<name>_dir.vpk`. File = preload ++ archive bytes. Read: all of it; CRC checked (0 = unrecorded). MD5 and
signature sections are not read or written.

## Binary KV3 (assets::kv3bin)

Versions 2-5, compression none / LZ4 (one raw block per buffer) / zstd; binary blobs (LZ4 blobs are chained
16 KiB frames), v5's two buffers and auxiliary-buffer typed arrays. Notable traps (from VRF's reader):
pre-v5 the type count includes the string bytes, and the 8-byte lane is aligned even when empty; v5 strings
are buffer 1's 1-byte lane with the count first in its 4-byte lane; v5 object member counts come first in
buffer 2. Legacy `VKV\x03` and v1 are not read. Flag bytes map to `resource`, `resource_name`, `panorama`,
`soundevent`, `subclass` (numbering from VRF's `KVFlag` usage, unverified beyond that).

## World (`world.vwrld_c`, DATA)

`m_worldNodes[].m_worldNodePrefix` (→ `<prefix>.vwnod_c`), `m_entityLumps[]` (resource names `.vents`).
Not read: `m_builderParams`, `m_worldLightingInfo` (lightmaps).

## World node (`.vwnod_c`, DATA)

Read: `m_sceneObjects[]` (`m_renderableModel` or `m_renderable`, `m_vTransform` = 3 rows of 4 floats, row i
= (rotation-scale row, translation i), `m_vTintColor` (alpha 0 means white), `m_nObjectTypeFlags`,
`m_flFade{Start,End}Distance`, `m_skin`, `m_nLODOverride` (-1 automatic)), `m_sceneObjectLayerIndices`,
`m_layerNames`, `m_aggregateSceneObjects[]` (`m_renderableModel`, `m_nLayer`, `m_anyFlags`, and
`m_aggregateMeshes[]` in the CS2 layout (VRF `SceneAggregate.cs`): `m_nDrawCallIndex`, `m_bHasTransform`,
`m_vTintColor` 0..255, `m_objectFlags`, `m_nLODGroupMask`; `m_fragmentTransforms[]` (3×4 like
`m_vTransform`) are taken in order by the meshes with a transform, the rest sit at identity: fragments are
world space). The older layout (meshes without `m_nDrawCallIndex`, draw call → mesh) draws the model whole.
Skipped (counted): `m_clutterSceneObjects`, `m_infoOverlays`. Not read: vis cluster membership, extra vertex
streams (baked vertex lighting), material overrides, lighting origins, aggregate `m_lodSetups`.

## World visibility (`world_visibility.vvis_c`: DATA + VXVS)

After VRF `VoxelVisibility.cs`. DATA: `m_nBaseClusterCount`, `m_nPVSBytesPerCluster`, `m_vMinBounds`,
`m_vMaxBounds`, `m_flGridSize`, `m_nSky/SunVisibilityCluster`, and six `{m_nOffset (from the VXVS block
start), m_nElementCount}` sub-blocks in order: `m_NodeBlock` (u32 pairs: leaf = first & 1, child/region
offset = first >> 1, region count = second & 0xFF, enclosed list = second >> 8, 0xFFFFFF none),
`m_RegionBlock` (u64: cluster = bits 0..15, bit 15 intersects geometry, leaf 16..40, mask index 40..),
`m_EnclosedClusterListBlock` (i32 offset, count), `m_EnclosedClustersBlock` (u16), `m_MasksBlock` (u64: a
leaf's 4×4×4 cells, bit x + 4y + 16z), `m_nVisBlocks` (PVS rows, `PVSBytesPerCluster` each, bit c LSB
first; the sky and sun rows sit at their cluster indices past the real clusters).
Octant order x = 1, y = 2, z = 4 (set when the point is past the middle). A point's cell mask is two more
halvings: `1 << (L1[o1] + L2[o2])`, L1 = [0,2,8,10,32,34,40,42], L2 = [0,1,4,5,16,17,20,21]. A point's PVS ORs
the rows of the matching regions; one in no region sees everything. Boxes walk the octree (grown by 1/32;
a box over the whole bounds is cluster 0; above 1024 on two axes, enclosed nodes give their cluster list).
DATA with `m_clusters` (region-box layout) or `m_visDataDwords` (BSP), and octrees of two clusters or fewer,
cull nothing: we treat them as no vis.

## Entity lump (`.vents_c`, DATA)

`m_name`, `m_childLumps[]` (loaded recursively; their entities follow the parent's in `GetEntityList`, a
judgment call: prefab instances compile to child lumps and managed only asks for `default_ents`),
`m_entityKeyValues[]`: either `keyValues3Data { version: 1, values {..}, attributes {..} }` or the legacy
`m_keyValuesData` blob: `u32 1, u32 valueCount, u32 attributeCount`, then `valueCount × (u32 keyHash,
u32 type, value)` and `attributeCount × (u32 hash, cstring name, u32 type, value)`. Types (EntityFieldType):
0x01 float, 0x03 vector, 0x05 int, 0x06 bool, 0x09 color32, 0x1A int64, 0x1E cstring, 0x21 uint64,
0x22 float64, 0x25 uint, 0x27 qangle. Key hash = `StringToken` (MurmurHash2, lowercased, seed 0x31415926).
`attributes` and `m_connections` (entity I/O) are not kept; managed reads neither.

## Engine side (crates/engine2/src/world)

`CreateWorld` opens the pak (a `MountWorldVPK` mount for the stem, else the path on the GAME search paths,
else loose files), mounts it into the native filesystem (`fs::read`), loads the map, and creates one
`CSceneObject` per world-node object through `model::load` (`m_nLODOverride` sets the object's LOD), or per
aggregate fragment over a runtime one-draw copy of the compiled model (fragment LOD masks aren't applied).
Bounds are the union of the placed model bounds. `SetWorldTransform` re-places objects.
`BuildPvs` reads `world_visibility.vvis_c` and always returns a valid `IPVS`: without usable vis it is empty
(`IsEmptyPVS`; managed's network culling then skips it) and every query answers visible. `IsInPVS` /
`IsAbsBoxInPVS` take managed's `Vector3*` sources: visible when any source's PVS holds the point's cluster
(a point in no cluster counts as visible) or any of the box's. Sky/sun queries test the point's cluster in
those rows. `ISceneWorld` keeps `SetPVS`'s PVS and the 3D skybox (`Get3DSkyboxOriginAndScale` is zero
until `Add3DSkyboxWorld`; `Remove3DSkyboxWorld` clears it whichever world is named). Nothing draws with
the PVS yet. `world.scene_c` reaches `MapInstance` through `ReadCompiledResourceFileJsonFromFilesystem`,
which reads mounted paks.
