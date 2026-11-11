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
  program.rs, state.rs   Program (one module or per-stage Stages) + pipelines cached by PipelineKey
                         (target formats, vertex buffers, raster/depth/blend state)
  placeholder.rs/.wgsl   the stand-in shader: position, optional color/normal (stride-0 defaults)
  pass.rs                execute(targets, load, draws): one render pass of MeshDraws
  mesh.rs, forward.rs    placeholder vertex layout; ForwardPass, Camera, DrawItem
  surface.rs             Surface on a SurfaceTarget or raw SDL handles; present = copy + present
  frame.rs               Frame: hash, compare with tolerance, golden hashes, PPM dump
crates/engine2/src/render   slots only; state lives in the objects/handle tables
  device.rs              device ownership, GetRenderDeviceAPI, flush/caps slots
  texture.rs, formats.rs CTextureBase: CPU copy + optional GPU texture; ImageFormat -> wgpu
  context.rs             IRenderContext: bound targets, viewport, scissor, attributes; clears
  buffers.rs             RenderBufferHandle_t table; RenderTools buffer upload/fill/copy
  swap_chain.rs, frames.rs   swap chains (surface or offscreen); UNDERBOX_FRAMES capture
  readback.rs            ReadTexturePixels sync/async, async buffer reads
  draw/                  gpu_mesh (MeshGlue upload by version), model (CModel -> draws),
                         immediate (SetRenderState/Draw*, context Draw*, DrawModel)
  scene/                 camera (CCameraRenderer), view, layer, setup (+RenderPipelineTools),
                         execute (layers -> draws), system (g_pSceneSystem frame slots)
  game_window.rs         GameWindowGlue attach; the client frame SourceEngineFrame runs
                         (managed OnClientOutput, then GameWindow.Present)
  shading/               material programs: program (translate Vs/Ps, layouts, uniform blocks,
                         vfx attributes), cache per (shader, combo) in mod.rs, vertex (shader
                         inputs <- mesh layout, defaults/instance slots), view (PerViewConstant
                         Buffer_t), layout (uniform block fill from naga), bind (bind groups by
                         reflected name), defaults (1x1 textures, samplers). Not yet used by draws.
  tools.rs               the remaining RenderTools/IRenderContext/g_pRenderDevice slots
  attributes, material, vfx, image, stats   (pre-M2 stand-ins)
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
- The device is surfaceless (`Gpu::headless`). A swap chain adds a `wgpu::Surface` on its SDL
  window from the same instance when `window::handle::native` gives handles; otherwise (SDL's
  offscreen driver, no window) it is two offscreen targets. Views always render into the swap
  chain's colour/depth textures; `Present` copies colour into the surface image.

## From scene objects to draw calls

The scene system already keeps worlds and objects (`engine2/src/scene`). Drawing follows the
native model one to one, so managed's pipeline code runs unchanged:

1. `CCameraRenderer::Render(swapChain)` / `RenderToTexture` builds a `Camera` from the generated
   fields managed set in `Configure` (position, angles, horizontal FOV, near/far, normalised
   viewport, ortho size per pixel, clip-space bounds, override projection, flips), creates an
   `ISceneView` and a `CRenderPipelineSetup`, and calls managed `AddLayersToView` (callback 34)
   when the renderer has a managed camera id. Afterwards it writes `RenderedProjectionToView`,
   `RenderedViewport` and `RenderedDepthRange`.
2. No layers added (no managed pipeline, as in tests) → `execute::default_layers`: clear
   (`clearColor`/`clearFlags` attributes, the colour linearized like managed's `ToLinear`), sky,
   opaque, all into an Rgba16Float HDR target per colour target, then `ResolveHDRToFinalSDR`.
3. The view runs immediately (`execute::run`), layer by layer on one context: Clear → load ops;
   Render → the worlds' objects that pass tags, layer match id and required/excluded flags (a model
   counts as `IsOpaque`); depth prepass, light binner setup and translucent model draws are
   skipped under the placeholder; managed objects call back `RenderObject` (47); Procedural →
   managed's `void(ManagedRenderSetup_t)` pointer; Blit → texture copy, or a `render::Blitter`
   draw when formats or sizes differ (ResolveHDRToFinalSDR: Rgba16Float → Rgba8Unorm). Then
   `OnSceneViewSubmitted` (36). `UNDERBOX_LAYERS=log` prints one line per layer (target, draws,
   draws with a material).
4. A model becomes one `Draw` per mesh draw call (LOD 0, enabled mesh groups): MeshGlue buffers
   upload by version (`draw/gpu_mesh.rs`). The call's material (loaded with the model, or the
   object's `SetMaterialOverrideForMeshInstances`) resolves to a program (`draw/shaded.rs`, see
   Shaders); without one, the mesh layout maps position/normal/color onto the placeholder.
5. `RenderTools.DrawModel`, `SetRenderState` + `Draw*`, `DrawSceneObject`, `DrawSkyBox` are the same
   path entered from managed with explicit state. `IMaterial2.GetMode` returns one
   `IMaterialMode` per material (`material_mode.rs`). `SetRenderState` draws shade with the
   mode's material, the state's attributes and the context's (`ContextBindings`), so managed's
   constant buffers and `BindTexture` slots reach the binder. `DrawModelDrawCall`
   (`draw/draw_call.rs`) draws one mesh draw call with managed's instance-id buffer as the
   instance stream and transforms from the `g_TransformBuffer` attribute; the indirect forms read
   their args back from the GPU buffer. Managed's culled draws only get an `InstanceCount` from
   `occlusion_cull_cs`, which runs on the GPU (`render/compute/occlusion.rs` is the CPU fallback
   when it doesn't build). `BlitTexture` copies or draws through `draw::blit` (scales, converts
   formats, applies `nSrgbTransfer`; swap-chain binds write sRGB like native).
   `BindDepthTarget`'s slice selects the array layer (cube face) draws and clears go to.

   Colour model (as native): textures declared `SrgbRead( true )` sample their sRGB view
   (decoded to linear; `render::Texture::srgb_view`), lighting is linear, and the encode happens
   once at output: Blit layers apply `RENDER_SRGB` (read decode, write encode) and the
   `D_SRGB_TRANSFER` combo (`draw::blit_transfer`).
   `draw/draw_call_tests.rs` is the headless repro: one box with managed-shaped view constants.
6. Scene objects own a copy of their model handle (`model::copy`): managed's `FromNative` wrappers
   destroy what they're handed, which used to empty the object.
7. Morphs (`draw/morph.rs`): an object's flex controller values run through each mesh's flex
   rules; targets named like a weighted descriptor move a cached CPU copy of the vertex buffer.
   GPU skinning and managed's morph/skinning slots: docs/skinning.md.
8. Dynamic objects (`scene/dynamic.rs`) draw their vertices as one call (`draw/dynamic.rs`), with
   gizmo lines and points expanded first (docs/skinning.md); aggregates draw each fragment as a
   draw call of their model's first mesh.

Contexts and views execute eagerly on the queue, so `Begin/FinishRenderingViews` only bracket the
frame (Finish prunes dead GPU meshes) and async readbacks complete inside the call. Display lists
and multi-threaded recording can come later without changing slot behaviour.

## Shaders

Placeholder WGSL (`crates/render/src/placeholder.wgsl`) until the translation pipeline lands
(research/shaders.md: `.shader_c` stores SPIR-V per combo; we compile the core `.shader` sources
HLSL → DXC → SPIR-V → naga → WGSL lazily per combo, cached). The seam is `render::Program`:

- `Program::new(gpu, label, wgsl, bind_group_layouts, entries)` takes one WGSL module;
  `Program::from_stages(gpu, label, vertex: Stage, fragment: Option<Stage>, layouts)` takes the
  translator's separate vertex and pixel modules. One `Program` per (shader, static combo, dynamic
  combo); spec constants are baked in before naga, so each combo is its own module.
- Bind by name from reflection, never by binding number: the SPIR-V passes renumber (bindless
  arrays move off set 1 binding 16; a texture sampled both ways gains `<name>_cmp`).
- Pipelines are cached per `Program` and `PipelineKey` (target formats, vertex buffer layouts,
  raster/depth/blend state). Sample count joins the key with MSAA.
- The builtin headers in `crates/shaders/hlsl/builtin/` (instancing, vertex decompression, morph,
  light probe, lightmap, fog) define buffer layouts the renderer must fill; several are stubs.
- `isnan(x)` is lowered to `x != x`: never enable fast-math in the backend.
- Expect naga to reject some DXC output (bindless arrays, `OpSpecConstantOp`, push constants);
  those combos fall back to the placeholder with a warning logged once per shader, never a crash
  (`Program::try_pipeline`, bind groups under `gpu.checked`).
- Core material shaders that ship without source (complex, generic, simple, skin, unlit,
  tools_sprite, ui, sky, ...) draw with a stand-in (`shading/standin.shader`, map in
  `standin.rs`): `g_tColor` × tint × vertex tint × the view's lighting (below), or unlit. Sky
  samples the sky material's `SkyTexture` cube along the view ray (`standin_sky.shader`); the
  gizmo shaders' geometry shaders are emulated (`standin_gizmo_*.shader`, docs/skinning.md).
  Pixel bindings that collide with a different
  vertex binding are moved apart after translation (`shading/stages.rs`).
- Binder precedence per uniform block (`shading/bind.rs`): managed's `SetConstantBufferData` bytes
  by block name, the view block, the lighting blocks, then member-wise attributes → material →
  annotation defaults. Textures: attributes → material → managed's sun cascade (the bindless
  index in its `DirectionalLightCB`, `shadow::managed_cascade`) → managed's local shadow slots
  (`shading/shadow_slots.rs`) → lighting (our shadow maps) → the
  context's `BindTexture` slot with that binding number. A texture that is the pass's own target
  is never bound (managed's shadow pass would sample the map it writes). `ComparisonFunc` and
  `RWTexture2D` (storage, mip 0, exact format) bind as declared.
- `UNDERBOX_DUMP_CB=<block>` logs every `SetConstantBufferData` of that block as words
  (`render/bindings.rs`): compare managed's bytes with ours before changing a layout.
- Bindless (`render/bindless.rs`): `GetTextureViewIndex` hands out linear/sRGB index pairs from
  1 (`GetTexture2DSrgb` adds 1); `g_bindless_*` arrays (with `TEXTURE_BINDING_ARRAY`) hold the
  real textures of each array's shape, the default elsewhere, partially bound up to the highest
  index; samplers are managed's sampler states (docs/skinning.md). naga can't compare-sample an array, so the stand-in reads
  managed's shadow maps through slots (`shading/shadow_slots.rs`). `render::Gpu` also takes
  `SUBGROUP` (wave ops in the lit shaders) and the adapter's per-stage binding limits.
- `SystemInfo.SetGpu` and `GetVideoMemoryInfo` report the largest Vulkan device-local heap
  (`Gpu::device_local_bytes`), which managed's quality preset detection reads.
- Textures load from compiled `vtex_c` first (`crate::compiled::texture`), then the ones
  `texture_generated.rs` draws (e.g. the dev test grid behind `testgrid.vmat`), else native's
  error texture. Materials without a `.vmat` source load from `vmat_c` (`crate::compiled::material`).
- Coverage over sbox-public's vmats: `sh tools/test.sh engine2 material_coverage -- --ignored`
  writes `target/material-coverage.tsv` (full wgpu error text per row). Main gaps: compute-only
  shaders and tools shaders without source (selection, visualisers). Translation status of the
  core shaders: research/shaders.md "Corpus status".

Compute, lighting and shadows (ours and managed's): docs/lighting.md.

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
  read back; GPU buffer upload, copy, read); `render/readback_tests.rs` the pixel reads.
- `render/scene/tests.rs` is the headless client frame without managed: a world with the stand-in
  cube and a sky, a `CCameraRenderer` configured through its fields, rendered to an offscreen swap
  chain with the default layers, presented, read back. It asserts sky/cube pixels, depth (cube
  < 1, sky = 1) and determinism, and checks `crates/engine2/tests/golden/frames.tsv`
  (`UNDERBOX_BLESS=1 sh tools/test.sh engine2 camera_draws` records).
- A booted client logs or dumps presented frames: `UNDERBOX_FRAMES=log` prints
  `frame: <swap chain> #<n> <w>x<h> <blake3>`, a directory writes PPMs;
  `UNDERBOX_FRAMES_EVERY=n` samples, `UNDERBOX_FRAMES_LAST=1` keeps one `<swap chain>.ppm`;
  `tools/ppm2png.py` converts.
- `tools/client-frame.sh` boots the client on `corpus/showroom` (camera, sun, three dev boxes on
  a plane), checks the last frames settled on one hash and compares it with
  `corpus/showroom/golden.tsv` per adapter (`UNDERBOX_BLESS=1` records); `target/client-frame/frame.png`.
  `MANAGED=1` does the same through managed's scene renderer (`golden-managed.tsv`); `CORPUS=x`
  picks another scene (`mapworld`, `skinned`).

## Windows and input (SDL)

`globalSdl` (126 slots, `engine/Definitions/engine/sdl.def`) is SDL3's API one function per slot,
plus Facepunch's `sbox/sdl_extensions.h`. Groups and their managed callers (`Core/Windowing`,
`Systems/Input`, `Systems/UI/Surface`):

| Group | Slots | Callers |
| --- | ---: | --- |
| Windows (create, state, size, fullscreen, hit test) | 35 | `SdlWindow`, `GameWindow`, `GameStartupWindow`, `SurfaceWindow` |
| Displays and modes | 10 | `SdlDisplay`, `SdlWindow.SetFullscreen` |
| Properties | 8 | `SurfaceWindow` (creation props), `PanelWindowDesktop` (`SDL.window.win32.hwnd`) |
| Events | 4 | `SdlEvents.Poll` (`PollEvent` into `Sdl.Event`), `PanelWindowInput` (event watch) |
| Keyboard, text input (IME) | 9 | `SdlEvents`, `KeyTranslation`, `WindowInput` |
| Mouse, cursors | 13 | `WindowInput`, `SdlCursors` |
| Surfaces (icons, cursor images, software splash) | 7 | `SdlCursors`, `SurfaceWindow`, `GameStartupWindow` |
| Gamepads | 14 | `SdlGamepads` (`gamecontrollerdb.txt`), `SdlEvents` |
| Clipboard, errors, hints, subsystems, video driver, screensaver, timing | 12 | `Clipboard`, `WindowInput`, `FramePacer` |
| Dialogs | 3 | `PanelWindowDialogs` |
| s&box extensions (drag/drop, taskbar, chrome) | 11 | `Drag`, `PanelWindowInput`, `SurfaceWindow` |

### Decision: link real SDL3, not winit

The `window` crate links SDL3 itself (built from `sdl3-src` with cmake in `crates/window/build.rs`,
static, audio/gpu/render/camera/tray off; bindings from `sdl3-sys` with `no-link`). Managed is
written against SDL3's semantics, not just its function names:

- `PollEvent` hands native managed's `Sdl.Event`, a 128-byte mirror of the `SDL_Event` union, and
  managed reads it by SDL's event ids and member layouts, including SDL-owned string pointers
  (text input, drops) valid until the next poll. `GetWindowFromEvent` takes the raw event pointer.
- Keys arrive as SDL scancodes and keycodes and `KeyTranslation.cs` maps those tables;
  `GetKeyName(GetKeyFromScancode(..))` must produce SDL's names for the active layout.
- Gamepads: `AddGamepadMappingsFromFile("core/cfg/gamecontrollerdb.txt")` is SDL's mapping
  database format (GUIDs, `platform:` filtering), and button/axis ids are SDL's `GameControllerCode`.
- IME composition (`SDL_IME_IMPLEMENTED_UI=composition`, `TextEditing` events), event watches and
  hit-test callbacks called from inside SDL's OS message pump, properties by SDL name
  (`SDL.window.create.*`, `SDL.window.win32.hwnd`), hints, `SDL_free` of SDL allocations,
  software window surfaces for the startup splash, and native file dialogs.

On winit every one of those is a re-implementation to keep bit-compatible with SDL (scancode and
keycode tables, layout-aware key names, a gamecontrollerdb parser plus a gamepad backend, IME
event shaping, an allocator managed can `Free`). SDL3 gives them exactly, so the slots are thin
forwards and fidelity is SDL's own. The cost is a C build (cmake, ~30 s once, cached) and that
SDL, not us, owns the platform layer; both match how native engine2 shipped.

### How it fits

- Handles are SDL's pointers. SDL validates window, gamepad and cursor pointers in its object
  registry, so a stale handle from managed fails with an SDL error instead of crashing.
- Video comes up lazily on the first call that needs it (`window::video::ensure`), through a gate
  engine2 installs: a dedicated server (`CMaterialSystem2AppSystemDict` flag) never brings video
  up, `PollEvent` returns false while no subsystem is initialised, and the clipboard falls back
  to an in-process copy. With no display, video falls back to SDL's `offscreen` driver: windows,
  framebuffers, properties and events all work, but there is no native surface.
- `SDL_WINDOW_VULKAN` is stripped at creation (SDL would require a Vulkan loader); presentation is
  wgpu's.
- Renderer seam: `window::handle::native(Window::from_ptr(ptr))` returns `NativeHandles`
  (`raw-window-handle` 0.6 window + display: Xlib, Wayland, Win32) for
  `wgpu::Instance::create_surface_unsafe`, or `None` (offscreen driver, macOS for now) meaning
  render offscreen. `CreateSwapChain` gets the `SDL_Window*` managed holds.
- Linux builds need cmake, `libx11-dev` and `libxext-dev`; each optional X11 extension (Xcursor,
  XInput2, Xrandr, ...) is compiled in only if its header exists (build warnings list the missing
  ones). Wayland is compiled in when its dev packages are present.
- Not yet: Windows-only extensions (OLE drag and drop, taskbar overlay, DWM chrome) are accepted
  no-ops or report "unsupported"; macOS native handles. `EngineGlobal.SourceEngineInitStartupWindow`
  (app/platform.rs) brings video up (`video::ensure`); managed then creates its windows itself.

Tests without a display (`tools/test.sh window`): layouts of every managed SDL mirror against
SDL3, event ids and flags; key, scancode and name tables; events pushed through SDL into managed's
struct; gamepad mapping files; the refused-video path (no display touched, clipboard in-process);
windows, framebuffers, properties and window events on the offscreen driver.

## Next

1. `BindVertexShader`/`BindPixelShader` (no `RenderShaderHandle_t` yet); `ctx::SET_DYNAMIC_
   CONSTANT_BUFFER_DATA` binds by register slot and has no caller in sbox-public.
2. Draw the 3D skybox world (the world lane stores it); apply the tonemap and volumetric fog
   parameters managed sets (docs/skinning.md); gizmo lines from GPU buffers.
3. Translucent and depth-prepass layers, MSAA, cube and stereo renders, GPU timestamps, mip
   generation.
