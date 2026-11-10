# Renderer (M2)

Our own renderer on wgpu, mirroring s&box's render-context and scene-object model (PLAN.md,
"Decisions"). This note covers what managed drives, how the pieces sit, and how it is tested
without a display. Paths under `engine/` are in sbox-public.

## What a managed client drives

| Surface | Slots | What managed does with it |
| --- | ---: | --- |
| `g_pRenderDevice` | 54 | `GetRenderDeviceAPI` once in `Graphics.Initialize` (`Systems/Render/Graphics.cs:22`): anything but Empty sets `Graphics.IsAvailable`, which gates particles, sprites, cubemaps, shader compiles and more. Swap chains (`SdlWindow.CreateSwapChain`, `Present`), textures (`FindOrCreateTexture2`, `ClearTexture`, `ReadTexturePixels`), GPU buffers (`CreateGPUBuffer`, `ReadBuffer`), standalone contexts (`CreateRenderContext`/`ReleaseRenderContext`) |
| `IRenderContext` | 41 | `Graphics.Context` inside a layer scope: `Clear`, `BindRenderTargets`, `SetViewport`, `Draw*`, `ReadTexturePixels` (async readback) |
| `RenderTools` | 63 | `SetRenderState` + context `Draw*` for `Graphics.Draw`; `DrawModel` (16 call sites) for meshes; buffer upload/fill/copy; `Compute`, `CopyTexture`, `ResolveFrameBuffer` |
| `CCameraRenderer` | 71 | `SceneCamera.AddToRenderList` → `CameraRenderer` configure → `Render(swapChain)` / `RenderToTexture` |
| `ISceneView`, `ISceneLayer` | 31, 35 | Native builds views; `NativeRenderPipeline.AddLayersToView` adds layers (`view.AddRenderLayer`); native calls back `Graphics.OnLayer` / `SceneCustomObject.RenderObject` per layer |
| `g_pSceneSystem` | 59 | `BeginRenderingViews`, `FinishRenderingViews`, `WaitForRenderingToComplete`, `SetMainSwapChain`, per-frame stats |
| `globalSdl` | 126 | Windows, displays, events, input, gamepads, cursors, clipboard (SDL3's API, one slot per function) |

Client frame (`EngineLoop.cs` ~340, `GameWindow.Rendering.cs:23`): `SetNextRenderTime` →
envmaps → `BeforeRenderingViews` → panel windows → `BeginRenderingViews` → `Game.Render`
(cameras `AddToRenderList`) → menu overlay (`CCameraRenderer.RenderOverlay`) →
`FinishRenderingViews` → `WaitForRenderingToComplete` → native calls managed `Present`.

## Modules

```
crates/render            wgpu, no abi: usable from tests and tools
  gpu.rs                 Gpu: instance, adapter, device, queue; Gpu::headless()
  texture.rs             Texture/TextureDesc: create, upload, clear, read_back -> Frame
  buffer.rs              Buffer/BufferKind: write, fill, copy_to, read
  program.rs             Program: WGSL module + bind-group layouts + pipelines per target format
  mesh.rs, forward.rs    placeholder vertex layout; ForwardPass, Camera, DrawItem
  frame.rs               Frame: hash, compare with tolerance, golden hashes, PPM dump
crates/engine2/src/render   slots only; state lives in the objects/handle tables
  device.rs              device ownership, GetRenderDeviceAPI, flush/caps slots
  texture.rs, formats.rs CTextureBase: CPU copy + optional GPU texture; ImageFormat -> wgpu
  context.rs             IRenderContext: bound targets, viewport, scissor, attributes; clears
  buffers.rs             RenderBufferHandle_t table; RenderTools buffer upload/fill/copy
  attributes, material, vfx, image, stats   (pre-M2 stand-ins, unchanged)
```

## Device ownership

One `render::Gpu` per process, in a `OnceLock` in `engine2/src/render/device.rs`.

- It is created only when managed asks `GetRenderDeviceAPI` and the app is neither a dedicated
  server nor a unit-test app (`CMaterialSystem2AppSystemDict` flags, set before `Graphics.Initialize`;
  `AppSystem.cs:402`), and `UNDERBOX_RENDER` is not `empty`. A server therefore never touches
  wgpu and keeps answering Empty, exactly as before.
- If no adapter exists the device stays empty (logged), so a client without a GPU degrades to the
  server behaviour instead of failing.
- Every other slot calls `device::gpu()`, which never creates: without a device textures stay CPU
  copies, buffer creation returns the null handle, clears are no-ops.
- The device is surfaceless (`Gpu::headless`). Swap chains will add a `wgpu::Surface` per SDL window
  from the same instance; offscreen rendering does not change.

## From scene objects to draw calls

The scene system already keeps worlds and objects (`engine2/src/scene`). Drawing follows the
native model one to one, so managed's pipeline code runs unchanged:

1. `CCameraRenderer::Render(swapChain)` / `RenderToTexture` creates an `ISceneView` for the camera
   and calls managed `NativeRenderPipeline.AddLayersToView`, which adds `ISceneLayer`s (depth
   prepass, opaque, translucent, post, UI) with their targets, viewport and shader mode.
2. `FinishRenderingViews` executes views in dependency order. For each layer: bind its targets on
   an `IRenderContext`, call back `Graphics.OnLayer` for managed hooks, then draw the layer's
   visible objects (world objects filtered by the camera's tags and frustum).
3. A scene object becomes draw calls through its model: each mesh draw call is (vertex/index
   buffer range, material, attributes). The material resolves to a `Program` for the layer's
   shader mode plus the object's combo values; per-object data (transform, tint, lighting) goes
   into a dynamic uniform slice, as `ForwardPass` does with its per-draw MVP + tint today.
4. `RenderTools.DrawModel`, `SetRenderState` + `Draw*` and `DrawSceneObject` are the same path entered
   from managed with explicit state.

Contexts execute eagerly on the queue (see context.rs). Display lists and multi-threaded
recording can come later without changing slot behaviour.

## Shaders

Placeholder WGSL (`crates/render/src/placeholder.wgsl`) until the translation pipeline lands
(research/shaders.md: `.shader_c` stores SPIR-V per combo; we compile the core `.shader` sources
HLSL → DXC → SPIR-V → naga → WGSL lazily per combo, cached). The seam is `render::Program`:

- `Program::new(gpu, label, wgsl, bind_group_layouts, entries)` takes any WGSL module with layouts
  built by the caller. The translator produces one `Program` per (shader, static combo, dynamic
  combo) with layouts from naga reflection; spec constants are baked in before naga, so each
  combo is its own module.
- Pipelines are cached per `Program` and target format. The key grows to include vertex layout,
  blend/depth/raster state and sample count as materials need them.
- Expect naga to reject some DXC output (bindless arrays, `OpSpecConstantOp`, push constants);
  those combos should fall back to the placeholder with a logged warning, never a crash.

## Headless path and frame-hash tests

- `Gpu::headless()` needs no display. On this Linux box wgpu picks **Vulkan on lavapipe**
  (`vulkan/llvmpipe (LLVM 21.1.8, 256 bits)`); `WGPU_BACKEND`, `WGPU_ADAPTER_NAME` pin another.
  CI Linux needs `mesa-vulkan-drivers`; Windows CI gets WARP through DX12.
- `crates/render/tests/frames.rs` draws known scenes at 64×64, reads them back and checks two things:
  1. **Reference comparison** (every driver): scenes built so edges fall on pixel boundaries are
     compared texel-for-texel with a CPU-drawn `Frame` (tolerance 1/255). Scenes without an exact
     reference (perspective) assert determinism (same scene, same hash) and coarse coverage.
  2. **Golden hash** (per driver): `tests/golden/frames.tsv` holds `name, adapter, blake3`. A
     recorded adapter must match exactly; an unrecorded one is skipped (logged).
     `UNDERBOX_BLESS=1 cargo test -p render -- --test-threads=1` records. A failing frame is
     written as PPM under `CARGO_TARGET_TMPDIR`.
- engine2's `render/context_tests.rs` drives the slots themselves (create texture, bind, clear,
  read back; GPU buffer upload, copy, read).
- The corpus will reuse `Frame` for client frame hashes once a client boots.

## Windows and input (SDL)

`globalSdl` is SDL3's API one function per slot; managed passes SDL types straight through
(`SDL_Event` written by `PollEvent`, window pointers into `CreateSwapChain`). The plan:

- A `window` crate (PLAN.md, Architecture) implements those functions on winit, keeping SDL3's
  event struct layout and ids: managed reads the bytes. Gamepads come from gilrs.
- Handles: `SDL_Window*` becomes a token mapped to a winit window; `CreateSwapChain` resolves it
  to a raw window handle and creates the surface from `Gpu`'s instance.
- Headless-testable without a display: properties, hints, clipboard (in-process), `GetError`,
  `DelayPrecise`, `InitSubSystem` bookkeeping, and event queue encoding. Window creation needs a
  display (or Xvfb in CI) and is tested there.
- winit wants the main thread on macOS; the engine loop already runs on managed's main thread, so
  event pumping happens in `PollEvent` rather than in a winit-owned loop (`pump_events`).

No SDL slot is bound by this lane yet (app/sdl.rs owns the two existing ones).

## Next

1. Swap chains: `CreateSwapChain`/`Present` on an offscreen texture when there is no window, so a
   client boots headless and every frame can be hashed.
2. `ReadTexturePixels` (device and async context) through `Texture::read_back`.
3. Vertex/index buffers (`MeshGlue`) and `RenderTools.SetRenderState` + `Draw*` on the placeholder
   program; then `DrawModel` from `CModel` meshes.
4. `ISceneView`/`ISceneLayer`/`CCameraRenderer` view execution as above, then the corpus drawn.
