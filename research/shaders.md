# Shaders: what `.shader_c` holds and how underbox should consume it

Answer: **yes, every compiled combo holds raw SPIR-V.** The DXIL path doesn't exist. Paths below are relative to `<sbox-public>/engine/`.

## Compile path (managed)
- The native compiler `vfx_vulkan.dll` (`VFX_DLL_001`) is loaded at `Sandbox.Engine/Systems/Render/ShaderCompile/ShaderCompile.cs:74-88`.
- Each combo is compiled with `native.CompileShader(ctx, staticCombo, dynamicCombo, vfx, VfxCompileTarget_t.SM_6_0_VULKAN, programType, ...)` (`ShaderCompile.cs:275`). DXC flags live in native and aren't visible from managed.
- Combos are enumerated as the static × dynamic product (`Resources/Shader/ShaderProgram.cs:32-34`) and grouped into `CVfxByteCodeManager` by static, then dynamic (`ShaderCompile/ProgramSource.cs:192-203`).
- `BuildCompiledShader` hands the result to native `WriteProgramToBuffer` (`ProgramSource.cs:210-215`).

## Per-combo blob (`CVfxShaderFileVulkan`)
`ShaderCompile/SpirvDump.cs:16-79`:
- `u32 version`: 4, or 5 when specialization constants are present.
- `i32 size`, followed by `size` bytes of **SPIR-V**, which goes straight to the driver.
- Variable-length reflection data.
- Version 5 only: a trailing `u32 count` and `count` × `(u32 id, u32 value)` pairs. These are the specialization-constant values for that combo.
- `SpirvDump.Specialize` (`:112-142`) bakes those values into `OpSpecConstant*` defaults. Many combos therefore share one canonical SPIR-V module and differ only by spec constants.

## `.shader_c` container
- The file is a Source 2 resource holding `DATA`, `SPRV` and optional `HLSL` blocks (`Tests/Sandbox.Test.Unit/Shaders/ShaderResourceFileTest.cs:9-37,53`). The block table is at `8 + i32@8`, with the count at `@12` and 12-byte entries.
- `SPRV` payload (`ShaderCompile/ShaderSource.cs:97-160`): `ProgramHeader_t[VFX_PROGRAM_MAX]` `{offset,size}` (offsets from the payload start, no length prefix: real files, docs/real-games.md), then per-program bodies. The `VFX_PROGRAM_FEATURE` body is the native feature/combo description; the other bodies are the `WriteProgramToBuffer` output.
- `HLSL` block: JSON `{Programs:[...]}` with each program's preprocessed source (`ShaderSource.cs:162-205`). It is only written for addon shaders; **core shaders have none** (`ShaderResourceFileTest.cs:68-72`).
- **Compression: zstd with an external dictionary** (real files, docs/real-games.md), so the shipped SPIR-V can't be decoded; the original note follows. The layout inside a program body (static-combo index, any LZMA/zstd) is written by native `WriteProgramToBuffer`, and managed never parses it. A grep for compress/zstd/lz4 across the shader code finds nothing. Source 2's `.vcs` historically compressed static-combo blocks, so plan for zstd/LZMA. Confirm against a real file.
- sbox-public's checkout contains no `.shader_c` files: they are gitignored and none were found on disk. No `.shader` sources ship in the repo either, so a real file can only come from a local s&box build. Nothing was downloaded.

## Combos and selection
- Static × dynamic counts are large. `Tools/SboxBuild/Steps/ShaderStats.cs:240` says "complex.shader alone is ~1900 big modules" after deduplication.
- Load passes `VFX_LOAD_STATIC_COMBO_DATA` to `CreateFromResourceFile(..., SM_6_0_VULKAN, ...)` (`Resources/Shader/Shader.cs:48,65`).
- At draw time, managed calls `SetCombo(token, value)` (`Systems/Render/CommandList/CommandList.Attributes.cs:183-193`), which becomes `CRenderAttributes::SetComboValue`. Native combines those values with material feature/static values to pick the static and dynamic indices. Under underbox, our side owns that selection: `CVfx_*`, `CVfxCombo`, `CVfxComboIterator` and `CVfxProgramData` in `crates/abi/src/generated/slots.rs:720-728,26058-26073`.

## Recommendation: (c), with shipped SPIR-V as the primary path
1. **Primary: parse the `SPRV` block in Rust, then run naga `spv-in` → WGSL/backend, lazily per (static, dynamic) combo, with a disk cache.** Before naga sees the module, apply the spec constants the way `SpirvDump.Specialize` does. naga can't handle these, and baking them avoids override plumbing.
2. **Fallback: DXC (`-spirv -T *_6_0`, the same target) → SPIR-V → the same naga path.** Use it only for addon shaders with an `HLSL` block that we need to recompile, and for shaders compiled from source in the editor. It doesn't need a separate WGSL toolchain.
3. **Known naga `spv-in` gaps with DXC output:**
   - bindless: runtime arrays of textures/samplers need `binding_array`, plus the wgpu features and limits for it;
   - `OpSpecConstantOp`;
   - some pointer/`OpCopyLogical`/struct-aliasing patterns;
   - push constants: wgpu `PUSH_CONSTANTS` exists only on native backends;
   - storage-image formats left `Unknown`;
   - `SubgroupBallot`/wave ops.

   Build a corpus test that runs every combo through naga validation and records the failures.
4. **wgpu SPIR-V passthrough** (`create_shader_module_passthrough`, Vulkan backend only) can serve as an escape hatch for modules naga rejects. It isn't portable and wgpu skips validation, so restrict it to a per-shader fallback when the backend is Vulkan.

Open items:
- Byte-level layout of `WriteProgramToBuffer` and `VFX_PROGRAM_FEATURE`, including compression. This needs a real `.shader_c` from a local build.
- Format of the reflection section, needed for bind group layouts.

## Source path as built (`crates/shaders`)
vfx parse → include expansion → HLSL → DXC (`tools/fetch-dxc.sh`, pinned) → SPIR-V passes → naga →
WGSL, plus reflection and a disk cache. Decisions, each with its reason:
- DXC args (`src/dxc.rs`): `-HV 2018` because the corpus uses per-component vector ternaries;
  `-fvk-use-gl-layout` because WGSL rejects DXC's relaxed and DX layouts.
- Slang-isms in the sources: `{}` init and `point` as an identifier are rewritten (`src/compat.rs`);
  `dynamic_uniform`/`asDynamicUniform` are defined away (`hlsl/prelude.hlsl`).
- Valve's closed headers are replaced by our own (`hlsl/builtin/`, see `hlsl/README.md`). The data
  headers fix a layout the renderer must follow.
- `SpecializedConstant()` globals become `static const T x = UB_SPEC_<n>;` and every variant
  compiles with its values defined. Real spec constants produce `OpSpecConstantOp`, which naga rejects.
- SPIR-V passes before naga, one module each:
  - `comparison`: a texture or sampler used both with and without depth comparison gets a `<name>_cmp`
    clone binding for the comparison uses;
  - `row_major`: non-square row-major struct members get the transposed type, because naga sizes them
    wrong;
  - `bindings`: aliased bindings (the bindless arrays share set 1 binding 16) move past the set's
    highest binding;
  - `rematerialize`: access chains used outside their block are cloned into it, because naga
    spills cross-block values into locals and a pointer can't be one (atmosphere_sky, terrain).
- Source rewrites before DXC: `compat` (Slang-isms, line-preserving) and `word_buffers`
  (`RWStructuredBuffer<S>` with `Interlocked*` on S's fields → a `uint` word buffer, since naga
  can't make a struct member atomic; occlusion_cull_cs), and `packed_buffers` (a structured buffer
  whose element std430 would pad, e.g. a `float3` member, → a `uint` word buffer with packed
  load/store helpers). Managed and native lay structured buffers out tightly (DX rules), but
  `-fvk-use-gl-layout` gives them std430: sprite_cs read `SpriteData.Splots` from another field and
  hung the GPU. The lit shaders' light, shadow, decal and DDGI buffers and the UI batches had the
  same mismatch. `tests/packed_audit.rs` lists what it rewrites and keeps.

  Reflection reports the final (group, binding) of each name, and the renderer binds by name.
- `isnan(x)` becomes `x != x` because WGSL has no `isnan` (`src/translate.rs`).
- Vertex and pixel stages are separate WGSL modules. `render::Program` takes one module, so the
  renderer lane needs a per-stage adapter. Nothing in `crates/render` was changed.

## Compiling through managed's ShaderCompile (`crates/engine2/src/shader/`)
`Shader.CompileAsync` / `CompileFileAsync` (GameShaders.cs) and ShaderUpgrade drive the same slots as
native's vfx_vulkan. Decisions:
- `IVfx.CompileShader` compiles one combo HLSL → SPIR-V with DXC (`Compiler::spirv`), from the
  context's masked source (`SetMaskedCode`, parsed once per context), else the CVfx's own parse.
  The blob is `CVfxShaderFileVulkan` v4 (version, size, SPIR-V, no reflection), what SpirvDump
  reads. DXC errors become `compilerOutput` with `compileFailed`; PS_RENDER_STATE compiles empty.
  WGSL translation is left to the renderer at draw time, so a combo naga rejects still compiles.
- `ShaderTools.MaskShaderSource` blanks other stage blocks (lines kept, `shaders::blocks::mask`).
- Versions (resource 4, VCS 65..68, ABI 2) come from ShaderUpgradeTest's `Current`, unchecked
  against a shipped engine2. Same ABI as native on purpose: native-compiled package shaders count
  as current and load from their HLSL block instead of being recompiled at install.
- `.shader_c` bodies: managed frames the container and SPRV index itself (`WriteResourceFile`
  outside the editor); each body starts with the VCS header ShaderUpgrade checks, then `UBX1`
  and ours: FEATURE = the parsed `VfxShader` as JSON plus the source hash, a stage = its combos
  `(static, dynamic, blob)`. Native's body layout is closed and unread.
- Loading (`CreateFromResourceFile`, `CreateAnonymous`, `GetShader` of a missing source): our
  FEATURE body, else the HLSL block's masked sources parsed per stage, else fail. A core shader
  without source or HLSL still has no shader (renderer stand-in, as before).
- `VFX_CHECK_MD5_AGAINST_SOURCE`: blake3 of the source text, its includes and our crate version;
  a mismatch (or a native file with a source beside it) is out of date, so managed recompiles.
- `compiled/...` (GameShaders' prefix) paths keep their folder (others get `shaders/`). Anonymous
  shaders register under their generated name, because the renderer finds shaders by path.
  `GetShader` reloads a shader whose `.shader_c` mtime changed, standing in for
  `mat_reloadshaders` (no native commands are announced yet). The renderer's program cache
  (render/shading/mod.rs) still keeps the old program until restart.
- `IsUiVariableActive` (hshader.def: the given feature at `value`, the others at their minimum)
  replays each program's `#if`/`#define` skeleton (`shaders::directives`) with a compile's defines
  and evaluates `ActiveIf( expr )`, also through a macro (`BENT_NORMAL_ACTIVE_IF`); inputs show
  only while a shown texture's `Channel` reads them (rules in shader/active.rs). Native's own
  traversal is closed: complex/skin (ShaderActiveIfTest) aren't in sbox-public, so their
  `BENT_NORMAL_ACTIVE_IF` definition is a guess. `.shader_c` FEATURE bodies older than this have
  no skeleton or lines and show everything declared.
- Not native's: `GetPropertiesJson` lists features, `CreateInput*` inputs and globals annotated
  with `Attribute`, `UiType` or `UiGroup` only.
- `IVfx.Init`/`ClearShaderCache`/`CreateSharedContext` stay in render/vfx.rs (render lane); its
  "compiles nothing" TODO is stale.

## Corpus status
`cargo test -p shaders --test corpus` runs each of the 85 core shaders' default combo (static 0,
dynamic 0) and writes `corpus.tsv`/`corpus-errors.txt` to CARGO_TARGET_TMPDIR. All 85 shaders
parse. The test's `MIN_OK_STAGES` is the regression floor; `corpus.tsv` has the current count.

Fixed by passes:
- `OpDecorateId` (`-fspv-reflect`'s append-buffer `CounterBuffer`) is dropped with the other
  reflection decorations (`clutter_cull_cs`).
- Storage-image arrays addressed by `uint3` (`ddgi_depth_copy_cs`, `envmap_filtering_cs`): naga
  only registers float coordinate vectors and panics looking up `vec2<u32>`; `image_coords.rs`
  declares the integer vectors.
- HLSL 2018's per-component `||`/`&&` on vectors becomes `|`/`&` on the naga IR
  (`translate::lower_vector_logic`), and `isfinite(x)` becomes `abs(x) <= FLT_MAX` in compat,
  because WGSL has no `isinf` (`screen_space_reflections_cs`).
- `SPV_KHR_compute_shader_derivatives` is stripped (`downsample_spd`).
- "invalid image [26]" (`particle_lighting_cs`): naga types a fetch's image by its variable, so
  DDGI's `Bindless::GetTexture3D(i).Load` saw naga type 26, the `g_bindless_Texture3D` binding
  array, not its element (the env-map cube was a wrong suspect: it's sampled, which naga
  unwraps). `bindless_fetch.rs` outlines each such fetch into a function taking the image.
  `ConstOffset` on a fetch is dropped by naga (WGSL's `textureLoad` has none).
- "unsupported storage class" (CMAA2's `InterlockedExchange`/`InterlockedAdd` on
  `RWTexture2D<uint>`): DXC emits `OpImageTexelPointer`, whose Image storage class naga rejects.
  `image_atomics.rs` moves such an image's texels into a `<image>_texel_words` storage buffer
  (rows padded to 256 bytes) and makes every texel access in that module a real atomic on it.
  Other modules use the image itself, so the renderer copies image → words before the dispatch
  and back after (engine2 `shading/texel_words.rs`). An atomic it doesn't know
  leaves the module unchanged.
- `UNDERBOX_DUMP_SPV=<dir>` writes the module naga gets per stage (cache misses only).

Remaining failures (`UNDERBOX_CORPUS_ONLY=a,b` runs only matching shaders):
- **DXC:** `ObjectHighlight` has a GS block without `MainGs`.
- **Geometry shaders:** `gizmo_line`, `gizmo_sprite`. These stay failures here because the corpus
  compiles their source; at runtime the renderer emulates them (docs/skinning.md).
