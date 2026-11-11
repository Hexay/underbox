# Source 2 compiled resource container (`*_c`) — reference for engine2 rewrite

Sources (read 2026-11-10):
- VRF = github.com/ValveResourceFormat/ValveResourceFormat @ `574c74f3`, MIT. Paths below are relative to its
  `ValveResourceFormat/` dir unless noted.
- SB = `<sbox-public>/engine` (local checkout).
- FIX = VRF `Tests/Files/sbox_bullet-wood.sbox_c` (906 bytes, a real s&box `.decal`-style managed resource shipped
  as an MIT test fixture). Hex offsets marked `FIX@0x..` were verified with `xxd`.
- Valve developer wiki: `developer.valvesoftware.com/wiki/Resource` returned HTTP 403 to automated fetch; nothing
  here is sourced from it.

All integers are **little-endian**. "Rel offset" = signed/unsigned 32-bit offset **relative to the address of the
offset field itself** (not the block start, not the file start).

## 1. File header (16 bytes)

| Off | Type | Field | Notes |
|----:|------|-------|-------|
| 0x00 | u32 | FileSize | whole file; FIX = 0x38A = actual length |
| 0x04 | u16 | HeaderVersion | must be **12** (`Resource.KnownHeaderVersion`, Resource/Resource.cs:34, checked :240) |
| 0x06 | u16 | Version | resource-type version; FIX = 1. s&box sets it via `ResourceCompileContext.ResourceVersion` → `SpecifyResourceVersion` (SB Sandbox.Tools/Assets/ResourceCompile/ResourceCompileContextImp.cs) |
| 0x08 | u32 | BlockOffset | rel offset to block table; **always 8** in practice (table starts at 0x10). Reader does `pos += blockOffset - 8` after reading it (Resource.cs:255); writer emits 8 (Resource.cs:439) |
| 0x0C | u32 | BlockCount | |

Reader rejects FileSize == VPK magic / legacy VCS magic before checking HeaderVersion (Resource.cs:228-243).

## 2. Block table (12 bytes per entry, starts at 0x10)

| Off in entry | Type | Field | Notes |
|----:|------|-------|-------|
| 0 | u32 | Type | FourCC, chars in file order: `'D' | 'A'<<8 | 'T'<<16 | 'A'<<24` (Resource/Enums/BlockType.cs) |
| 4 | u32 | Offset | rel offset **from this field** (entry+4) to block payload (Resource.cs:263-264) |
| 8 | u32 | Size | payload bytes |

Payload address = `entryAddr + 4 + Offset`. FIX: DATA entry at 0x28, offset field 0x2C holds 0x194 → payload 0x1C0.

**Alignment.** VRF's writer pads each payload to 16 bytes and (cutely) writes `S2V` inside padding ≥5 bytes
(Resource.cs:453-480) — that is VRF-specific, not Valve's. Valve's own FIX layout: table ends 0x34, RERL at 0x34
(no pad), RERL ends 0xA5, REDI at **0xA8** (pad to 4), DATA at 0x1C0. So Valve aligns blocks to **≥4 bytes, zero
padding** (possibly 16 for some block types; unverified). Readers must use the table offsets, never assume packing.

Block order seen in FIX: `RERL, REDI, DATA`. VRF throws on any FourCC not in its enum (`ConstructFromType`,
Resource.cs:~545 `_ => throw`), so VRF cannot open s&box files that carry custom blocks (`LIPS`, the blob block).

## 3. Known block types (VRF Resource/Enums/BlockType.cs, Resource.cs ConstructFromType)

| FourCC | Meaning | Payload encoding |
|---|---|---|
| RERL | external resource refs | struct list, §3.1 |
| REDI | edit info (legacy, NTRO-era) | 10 sub-lists, §3.2 |
| RED2 | edit info v2 | binary KV3, same data keyed by name, §3.3 |
| NTRO | introspection manifest (struct schemas for NTRO-typed DATA) | deprecated by Valve; read-only concern |
| DATA | the resource payload | type-dependent: KV3, NTRO struct, KV1, or **plain bytes** (s&box GameResources) |
| STAT, INSG, SrMa, LaCo, FLCI, DSTF, CTRL | misc | binary KV3 |
| VBIB, MBUF, MVTX, MIDX, MADJ, MSLT, TBUF, VXVS, SNAP, MDAT, MRPH, ANIM, ASEQ, AGRP, PHYS | mesh/anim/phys | type-specific |

DATA dispatch (Resource.cs:266-286): for types VRF doesn't special-case, it peeks the first u32 of DATA; if it's a
KV3 magic → BinaryKV3, if KV1 magic → BinaryKV1, else by resource type. Resource type comes from file extension, or
REDI/RED2 `SpecialDependencies[].CompilerIdentifier` (Resource.cs:301-322, 667-720).

### 3.1 RERL (Blocks/ResourceExtRefList.cs)

| Off | Type | Field |
|---|---|---|
| 0 | u32 | rel offset to entry array (VRF writes 8; **Valve FIX writes 0x0C** → entries 8-aligned at RERL+0x0C) |
| 4 | u32 | count |
| entry+0 | u64 | resource id |
| entry+8 | u32 | rel offset (from this field) to NUL-terminated UTF-8 path |
| entry+12 | u32 | 0 padding |

Strings follow the entry array. FIX@0x40: id `0x88D0B06C44A4D9D6` → `materials/decals/wood-impact.vmat`.
**Id hash unverified**: I tested MurmurHash64A of the path (with/without `_c`) under seeds 0xEDABCDEF, 0x31415926,
0 — none matched FIX. Open question.

### 3.2 REDI (Blocks/ResourceEditInfo.cs, Blocks/ResourceEditInfoStructs/*.cs)

Header = 10 × `{u32 relOffset, u32 count}`; each relOffset is from the start of that 8-byte pair. Order:
InputDependencies, AdditionalInputDependencies, ArgumentDependencies, SpecialDependencies, CustomDependencies,
AdditionalRelatedFiles, ChildResources, SearchableUserData-int, -float, -string. All "str" below are rel-offset
strings (`ReadOffsetString`, Utils/StreamHelpers.cs:73: i32 rel offset, 0 = empty, field is 4 bytes).

| List | Element layout | Size |
|---|---|---|
| Input / AdditionalInput | str ContentRelativeFilename, str ContentSearchPath, u32 FileCRC, u32 flags (1=Optional, 2=FileExists, 4=IsGameFile) | 16 |
| Argument | str ParameterName, str ParameterType, u32 Fingerprint, u32 FingerprintDefault | 16 |
| Special | str String, str CompilerIdentifier, u32 Fingerprint, u32 UserData | 16 |
| AdditionalRelated | str ContentRelativeFilename, str ContentSearchPath | 8 |
| ChildResources | u64 id, str name, u32 pad | 16 |
| Searchable int/float/string | str key, then i32 / f32 / str | 8 |

FIX REDI (FIX@0xA8..0x1C0), decoded:
- Input: `decals/bullet-wood.decal`, searchpath `base`, CRC 0xC23D60FF, flags 2.
- Argument: `___OverrideInputData___` / `BinaryBlobArg`, 0, 0.
- Special: String `Sbox Data Compiler Version`, CompilerIdentifier **`ManagedResourceCompiler`**, fingerprint 1, userdata 0.
- Searchable int: `IsChildResource` = 0.

### 3.3 RED2 (Blocks/ResourceEditInfo2.cs)
Whole payload is one binary KV3 document (§4). Root keys: `m_InputDependencies`, `m_AdditionalInputDependencies`,
`m_ArgumentDependencies`, `m_SpecialDependencies`, `m_AdditionalRelatedFiles`, `m_ChildResourceList` (string[]),
`m_WeakReferenceList` (string[]), `m_SearchableUserData` (object), `m_SubassetReferences`, `m_SubassetDefinitions`.
Element field names: see the KV constructors in Blocks/ResourceEditInfoStructs/*.cs (e.g. `m_CompilerIdentifier`;
names not individually verified here). Whether current s&box emits REDI or RED2: **unverified** (FIX is REDI; its
age is unknown). A reader must accept both.

## 4. Binary KV3 (Resource/ResourceTypes/BinaryKV3*.cs)

### 4.1 Magic (first u32 of payload; BinaryKV3.cs:50-82)
| u32 | bytes on disk | version |
|---|---|---|
| 0x03564B56 | `56 4B 56 03` "VKV\x03" | 0 (legacy, BinaryKV3.Legacy.cs) |
| 0x4B563301..05 | `01 33 56 4B` … `05 33 56 4B` | 1..5 (`magic & 0xFFFFFF00 == 0x4B563300`, low byte = version) |

VRF can **write only v4 and v5** (Serialization.cs:83); it re-emits v1-3 as v5.

### 4.2 Header (after magic; ReadHeader, BinaryKV3.cs:~409-480)
| Field | Type | Versions |
|---|---|---|
| Format GUID | 16 bytes (.NET Guid byte order) | all; e.g. `generic` = `7C 16 12 74 E9 06 98 46 AF F2 E6 3E B5 90 37 E7` (Serialization/KeyValues/KV3IDLookup.cs:59) |
| CompressionMethod | u32: 0 none, 1 LZ4 (raw block, not frame), 2 zstd | all |
| v1 only: CountBytes1, CountBytes4, CountBytes8, SizeUncompressedTotal | 4×i32 | 1 |
| CompressionDictionaryId | u16, must be 0 | ≥2 |
| CompressionFrameSize | u16: 16384 if LZ4 else 0 (reader enforces) | ≥2 |
| CountBytes1, CountBytes4, CountBytes8, CountTypes | 4×i32 | ≥2 |
| CountObjects, CountArrays | 2×u16 (saturating hints) | ≥2 |
| SizeUncompressedTotal, SizeCompressedTotal, CountBlobs, SizeBlobBytes | 4×i32 | ≥2 |
| CountBytes2, SizeBlockCompressedSizesBytes | 2×i32 | ≥4 |
| SizeUncompBuf1, SizeCompBuf1, SizeUncompBuf2, SizeCompBuf2 | 4×i32 | 5 |
| CountBytes1Buf2, CountBytes2Buf2, CountBytes4Buf2, CountBytes8Buf2 | 4×i32 | 5 |
| CountNodes, CountObjectsBuf2, CountArrays, CountArrayElements | 4×i32 (hints, except ObjectsBuf2) | 5 |

v5 header-level counts describe buffer 1 (VRF writes CountBytes1 = string bytes, CountBytes4 = 1, CountBytes8 = 0,
CountBytes2 = 0; Serialization.cs SerializeVersion5). Uncompressed v5: VRF writes SizeCompBuf1/2 = 0 and the reader
ignores them for method 0. SizeCompressedTotal = compressed buf1+buf2 (+blobs only for zstd).

### 4.3 Buffers
Lanes are concatenated, each aligned to its element size **relative to buffer start**, empty lanes skipped
(pre-v5 always aligns to 8 after the 8-byte lane even when empty; BinaryKV3.cs ReadBuffer).

v5 (two buffers, each compressed separately, then blobs):
- **Buffer 1**: `[bytes1 = strings, NUL-terminated UTF-8, concatenated][align4][i32 stringCount]` (+ aux 2/4/8 lanes
  for `ARRAY_TYPE_AUXILIARY_BUFFER`; VRF never writes those).
- **Buffer 2**: `[i32 objectMemberCount × CountObjectsBuf2][bytes1][bytes2][bytes4][bytes8][types: CountTypes bytes]
  [i32 blobLength × CountBlobs][u32 0xFFEEDD00][u16 lz4 frame size × N]`.
- Then blob bytes (raw / chained-LZ4 frames of ≤16384 / one zstd stream), then `u32 0xFFEEDD00` **only if CountBlobs>0**.

v2-v4 (one buffer): `[bytes1][bytes2 (v4)][bytes4: stringCount first, then values][bytes8][strings][types]`
then trailer `0xFFEEDD00` if no blobs, else blob lengths + trailer + lz4 sizes; then blobs + trailer.
Object member counts live in bytes4 inline. v4 zstd: buffer+blobs are one stream.

### 4.4 Types and values (BinaryKV3.NodeType.cs, ReadValue/ReadType)
Type byte (v≥3): low 6 bits = type; bit 0x80 → next type byte is a KVFlag; bit 0x40 → skip one extra byte. v1-2:
0x80 = legacy flag byte, low 7 bits type. Writer: `type|0x80, flag` only if flag≠0 (Serialization.cs WriteType).
KVFlag numeric values come from the ValveKeyValue NuGet package (`None, Resource, ResourceName, Panorama, SoundEvent,
SubClass`, …) — **exact numbers unverified here**.

| # | Type | Payload lane |
|---|---|---|
| 1 NULL, 13 TRUE, 14 FALSE, 15 INT64_ZERO, 16 INT64_ONE, 17 DOUBLE_ZERO, 18 DOUBLE_ONE, 26 UINT32_ZERO, 27 UINT32_ONE | none |
| 2 BOOLEAN, 22 INT8, 23 UINT8 | bytes1 |
| 20 INT16, 21 UINT16 | bytes2 |
| 11 INT32, 12 UINT32, 19 FLOAT | bytes4 |
| 3 INT64, 4 UINT64, 5 DOUBLE | bytes8 |
| 6 STRING | bytes4: i32 string index (−1 = "") |
| 7 BINARY_BLOB | v1: bytes4 length + bytes1 data; v≥2: next blob from blob table |
| 8 ARRAY | bytes4: i32 count, then count × (type, value) |
| 9 OBJECT | count (v5: objectLengths lane; else bytes4), then count × (type, bytes4 name-string-index, value) |
| 10 ARRAY_TYPED | bytes4 count; 24 ARRAY_TYPE_BYTE_LENGTH / 25 ..._AUXILIARY_BUFFER: bytes1 count. Then ONE element type, then count values (never empty in v≥2) |

Decode order: read root type from the types stream, then recurse; every value consumes from its lane in order.
Types are consumed in DFS order: an object's member types interleave with names (bytes4) and values.

**Simplest valid writer**: v5, CompressionMethod 0, no blobs, no typed arrays, no flags. Readers (VRF and, presumably,
the engine) accept method 0 (`ReadBufferData` case Uncompressed). Whether Facepunch's engine ever *emits*
uncompressed KV3 — **unverified**; byte-identical output vs. their compiler would need their method (likely LZ4).

## 5. s&box specifics (SB)

- **JSON location: the DATA block is raw UTF-8 JSON, NUL-terminated.** `AssetSystem.CompileGameResource`
  (SB Sandbox.Tools/Assets/AssetSystem.Compile.cs:114-149) does `context.Data.Write(jsonString)`, and
  `DataStream.Write(string)` appends a 0 byte (SB Sandbox.Engine/Resources/Compiling/ResourceCompileContext.cs ~L253).
  FIX confirms: DATA @0x1C0 size 0x1CA = `{"Decals":[...]}` + `00`, ending exactly at FileSize. No KV3 wrapper.
  The JSON is post-`ScanJson` (paths rewritten/embedded resources compiled) and includes `__version` if
  `ResourceVersion` > 0 (GameResource.cs Serialize/JsonUpgrade).
- `ReadCompiledResourceFileJson(ptr,len)` therefore ≈ "find DATA, return as string (strip trailing NUL)". Null return →
  managed throws `InvalidDataException` (ResourceLibrary.cs:510-517). Inferred, native is closed.
- Legacy pre-Feb-2023 KV3-text sources (`<!-- kv3 ...`) are converted to JSON at compile time (AssetSystem.Compile.cs:123-129);
  doesn't affect `_c` layout.
- **Blob block name**: `BlobDataSerializer.CompiledBlobName = "DBLOB"` (SB Sandbox.Engine/Utility/BlobDataSerializer.cs:13)
  — **five characters**, passed unvalidated to native `IResourceCompilerContext.WriteBlock` and
  `EngineGlue.ReadCompiledResourceFileBlock` (AssetSystem.Compile.cs:145, SceneResourceCompiler.cs, GameResource.cs:436,
  SceneFile.Load.cs:41). The public API path (`ResourceCompileContext.WriteBlock`, `ResourceLoadContext.ReadData`) enforces
  length 4; the blob path bypasses it. The on-disk FourCC is decided by closed native code: most likely the first 4 bytes,
  **`DBLO`** (if native does `*(uint32*)name`), but **unverified**. Source of blob payload: the `<file>_d` sidecar or a
  scene's base64 `__blobdata`; payload format is BlobDataSerializer's (8-byte header + 32-byte TOC entries; see that file).
- Custom blocks: any 4-char code via `ResourceCompileContext.WriteBlock/WriteBlockJson` (e.g. `"LIPS"` = UTF-8 JSON, no NUL;
  empty data is not written). Read by `ResourceLoadContext.ReadData/ReadJson`.
- References: `AddRuntimeReference` → native `RegisterReference` (→ RERL, inferred); `AddCompileReference` →
  `RegisterInputFileDependency` (→ REDI/RED2 input deps, flag 1 = optional); `AddGameFileReference` →
  `RegisterAdditionalRelatedFile_Game`.
- **How VRF identifies s&box files**: `ResourceType.SboxManagedResource` (Resource/Enums/ResourceType.cs:~371,
  extension attr `sbox`) chosen when a special dependency's CompilerIdentifier is `ManagedResourceCompiler` or `SBData`
  (Resource.cs:699); such DATA blocks are parsed as `Plaintext` (Resource.cs:629; ResourceTypes/Plaintext.cs reads all
  `Size` bytes as UTF-8, so the trailing NUL ends up in the string). Native types (`.vmat_c`, `.vtex_c`, `.sound_c`) go
  through VRF's normal Valve paths (Material/Texture/Sound blocks).

## 6. Smallest valid `.scene_c` we could write (recipe)

Goal: loadable by our engine and VRF-parseable, structurally matching FIX. Little-endian throughout.

1. Header: `FileSize` (patch at end), `u16 12`, `u16 Version` (scene's ResourceVersion; FIX uses 1), `u32 8`, `u32 N`.
2. Block table, N entries, blocks in order **RERL, REDI, DATA[, DBLO?]**.
   RERL may be `{u32 8, u32 0}` if no refs (8 bytes). Valve uses 0x0C as the entry offset; either is valid to VRF.
3. REDI minimal: 80-byte header of ten `{0,0}` pairs, plus one SpecialDependency
   `{"Sbox Data Compiler Version", "ManagedResourceCompiler", 1, 0}` so VRF classifies it as SboxManagedResource.
   (An empty RED2 also works for VRF: `Size==0` is accepted, ResourceEditInfo2.cs Read; but then VRF can't type it
   without the `.sbox_c`/known extension.) Optionally one InputDependency for `scenes/x.scene`.
4. DATA: UTF-8 JSON of the scene + `00`. Size includes the NUL.
5. Pad each block start to 4 bytes with zeros (16 is also safe); offsets are rel to each entry's offset field.
6. Only if the scene has `$blob` refs: append the blob block (FourCC per open question below). Without it, managed code
   accepts an empty table when the JSON contains no `"$blob"` (GameResource.cs:436-437).
7. Patch FileSize.

Validation: VRF CLI (`Source2Viewer-CLI -i x.scene_c --block DATA`) should print the JSON; round-trip FIX through our
writer and diff — expected differences are RERL entry offset (8 vs 0x0C) and padding unless we copy Valve's choices.

## Open questions
1. RERL/child-resource u64 id hash function (not MurmurHash64A with seeds 0xEDABCDEF/0x31415926/0 over the path).
2. Exact FourCC native writes for `"DBLOB"` (`DBLO`? `BLOB`?) — needs one real blob-bearing `.scene_c`/`_d`-backed file.
3. REDI vs RED2 in current Facepunch compiler output; header `Version` value for scenes/prefabs.
4. Valve's block alignment rule (4 vs 16) and which KV3 compression Facepunch's native writer uses (for byte-identity).
5. KVFlag numeric values (ValveKeyValue package) — read that package before emitting flagged KV3 strings
   (resource refs in `.vmat_c` use them).
