# underbox — plan

An open Rust replacement for s&box's closed native core (`engine2`), at 100% of the runtime
interop table `igen_engine`, so the MIT C# engine (Facepunch/sbox-public) and games built on it
run unchanged with no closed binary under them.

Prior art: AnthonE/engine-shim (Rust, started 2026-10-29). As of 2026-11-10 about 30% of its
slots are hand-written, 18% are generated store-and-return accessors and about 52% are stubs.
We keep its two sound ideas (a generated slot table, and trace-driven discovery of which slots
games call) and fix the rest; see "Lessons from engine-shim".

## Decisions

| Area | Decision |
| --- | --- |
| Scope | `igen_engine` + the `VFX_DLL_001` interface. The editor tables (`igen_tools`, assetsystem, Hammer, ModelDoc, AnimGraph) and lighting baking are out of scope; creators author in real s&box |
| Correctness | Per-slot contracts with tests, a CI game corpus, and differential runs against real engine2 (blocked until the EULA is confirmed) |
| Platform services | One `Platform` trait: Steam (steamworks-rs), offline/dev, custom. The core never names a backend |
| Renderer | Our own, on wgpu, mirroring s&box's render-context and scene-object model one to one |
| Shaders | Ahead-of-time translation: vfx blocks and combos parsed in Rust, HLSL to SPIR-V (DXC/Slang), naga to WGSL, cached |
| Language | Engine code is Rust; open C/C++ through FFI where it gives exact fidelity (Box2D v3, DXC/Slang, steam_api) |
| Platforms | Linux + Windows in CI. Don't design out macOS (main-thread windowing) or wasm |
| Order | M0 foundation → M1 headless server → M2 client → M3 long tail |

## Closed modules and where they sit

Checked in sbox-public 2026-11-10. The C# loads only `engine2` (the `igen_engine` table),
`vfx_vulkan` and `filesystem_stdio` (both through `CreateInterface`, in `ShaderCompile.cs`), and
`steam_api64` (loaded directly, plus `[DllImport]`). Every other closed module is reached only through
`igen_engine` slots (`engine.def` includes `engine/*`, `common/*` and `resources`), so it is in scope
as the code behind those slots:

| Closed module | Ours |
| --- | --- |
| engine2 (loop, fs, net, physics glue, nav, video, Steam glue) | `engine2` cdylib + subsystems |
| rendersystem, vfx_vulkan (runtime) | `render` |
| vfx_vulkan `VFX_DLL_001`, shadercompiler, shaderpreprocessing | `shaders`, exported through our `CreateInterface` |
| resourcecompiler (`IResourceCompilerSystem`), bitmap, modellib, quickhull | `assets` (runtime builders: `ModelBuilder`, hulls, bitmaps) |
| soundsystem | `audio` |
| animgraph | `anim` |
| steamaudio | FFI to the Steam Audio SDK (Apache-2.0) |
| ovrlipsync (Oculus SDK, closed) | our own viseme estimator |
| steam_api64 | `platform` |
| schemacompiler | build-time only; needed only if a runtime slot exposes schema data (to check) |
| lightinglib, vrad3, embree | **out of scope**: Hammer baking; runtime reads baked results |
| hammer, met, modeldoc_editor, animgraph_editor, toolframework2, assetsystem | **out of scope**: editor tables |

## Architecture

Cargo workspace. Each arrow points at a dependency, and there are no cycles.

```
engine2 (cdylib)  ── igen_engine/CreateInterface exports only; wires slots to subsystems
  └─ abi           ── generated: slot table, #[repr(C)] structs, names, hash. All FFI unsafe lives here
      └─ core      ── handle registry (typed Handle<T>, generational), managed callbacks,
                      string ownership (no leaks: arena per frame or refcounted), slot status registry
subsystems (each depends on core, never on each other except through traits):
  physics    ── Rapier (3D) + Box2D v3 via FFI (2D)
  scene      ── scene worlds and objects, lights, sky, decals, probes
  render     ── wgpu device, render contexts, attributes, buffers, textures, cameras
  shaders    ── vfx parser + translation pipeline (also a CLI for ahead-of-time builds)
  assets     ── Source 2 resources: KV3, models, anims, meshes, textures, materials, VPK, maps
  audio      ── mixer, voices, Steam Audio effects (SDK via FFI), lip-sync visemes
  anim       ── animation graphs, skeletons, clips, IK
  net        ── Glue.Networking sockets and poll groups over a pluggable transport
  platform   ── Platform trait + backends (steam, offline, …)
  window     ── SDL3-shaped windows and events on winit
tools/gen    ── generator (see below)
tools/corpus ── game corpus runner + scenario watchers
```

Rules:
- `unsafe` only in `abi` and FFI wrappers. Subsystems receive safe Rust types (`&str`, slices,
  `Handle<T>`), and pointers never travel as `usize`.
- One module per engine class family; no file over 300 lines. Slot implementations are
  registered by attribute (`#[slot("CPhysicsBody_SetVelocity")]`), not found by naming convention
  in one god file.
- Subsystem state lives in an `Engine` context owned by `core`, not in global statics.

## Generator

Inputs: sbox-public's `engine/Definitions/*.def` (slot order, via InteropGen's rules) and a small
C# tool that reflects the built `Sandbox.Engine.dll` and emits JSON (argument and return types,
struct layouts, managed `sizeof`). A Rust generator turns the JSON into `abi`. The C# is only a
layout dumper.

Each slot has a status, recorded in the generated table and in the coverage report:

`Stub` → `Stored` (generated get/set) → `Implemented` → `Contracted` (contract test passes)
→ `Differential` (matches real engine2).

"100%" means every slot is at least `Contracted`.

## Correctness

1. **Contracts.** For each slot, what the C# expects: its call sites in sbox-public, its
   preconditions, its return-value semantics. Kept as a test next to the implementation.
2. **Corpus.** Open-source games (Sweeper, Facepunch/sandbox, apetavern/grubs, and more as found)
   run headless in CI with scripted scenarios and assertions (positions, events, frame hashes for
   the client).
3. **Differential.** Blocked until the user confirms the s&box EULA permits running real engine2
   for comparison.

## Tracking upstream

A nightly CI job checks out sbox-public's latest commit, regenerates, and opens a report: slots
added, removed or retyped, and the interop hash. Generated code is never edited by hand.

## Milestones

- **M0 foundation:** workspace, generator, `abi`/`core`, handle registry, slot status and coverage
  report, CI (Linux + Windows), and sbox-public's dedicated server booting on all-stubs.
- **M1 headless server:** physics (3D + 2D), assets, scene (server-side), net, platform (offline +
  Steam), animation. The corpus hosts and plays scripted matches.
- **M2 client:** window/input, render, shader pipeline, audio, UI. The corpus is drawn, with
  frame-hash tests.
- **M3 long tail:** VR (OpenXR), video, HTML surface, UGC, and every remaining slot to `Contracted`.

## Lessons from engine-shim

- Raw `usize` pointers, leaked `CString`s and about 93 global mutex statics: replaced by typed
  handles, owned strings and an `Engine` context.
- A 3.8k-line `impls.rs` holding about 1,000 slots: replaced by per-family modules and slot attributes.
- Its store and Ethereum sign-in built into the core: replaced by the `Platform` trait.
- Shaders hand-ported to WGSL: replaced by the translation pipeline.
- "It didn't crash" as the test: replaced by contracts and corpus assertions.
- The README doubling as a changelog: replaced by a lean README; history lives in git.

## Open

- s&box EULA: does it allow differential testing against engine2? (user to confirm)
- Content licences: Facepunch's Citizen models and sbox.game packages are under Facepunch's
  terms; each corpus game needs its creator's terms checked.
- Do compiled `.shader_c` files hold SPIR-V (`ShaderCompile.cs` targets `SM_6_0_VULKAN`)? If so,
  runtime can translate shipped bytecode with naga directly, and the HLSL path is only for
  compiling from source.
- Does any runtime slot need schemacompiler's output?
- Project name ("underbox" is the working name; never use "s&box" as ours).
