# Shaders: what `.shader_c` holds and how underbox should consume it

Answer: **yes, every compiled combo holds raw SPIR-V.** The DXIL path doesn't exist. Paths below are relative to `/srv/agents/work/sbox-public/engine/`.

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
- `SPRV` payload (`ShaderCompile/ShaderSource.cs:97-160`): `u32 len`, then `ProgramHeader_t[VFX_PROGRAM_MAX]` `{offset,size}`, then per-program bodies. The `VFX_PROGRAM_FEATURE` body is the native feature/combo description; the other bodies are the `WriteProgramToBuffer` output.
- `HLSL` block: JSON `{Programs:[...]}` with each program's preprocessed source (`ShaderSource.cs:162-205`). It is only written for addon shaders; **core shaders have none** (`ShaderResourceFileTest.cs:68-72`).
- **Compression: unverified.** The layout inside a program body (static-combo index, any LZMA/zstd) is written by native `WriteProgramToBuffer`, and managed never parses it. A grep for compress/zstd/lz4 across the shader code finds nothing. Source 2's `.vcs` historically compressed static-combo blocks, so plan for zstd/LZMA. Confirm against a real file.
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
