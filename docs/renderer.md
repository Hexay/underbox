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
  (app/platform.rs) still answers false, which a windowed client needs to be true.

Tests without a display (`tools/test.sh window`): layouts of every managed SDL mirror against
SDL3, event ids and flags; key, scancode and name tables; events pushed through SDL into managed's
struct; gamepad mapping files; the refused-video path (no display touched, clipboard in-process);
windows, framebuffers, properties and window events on the offscreen driver.

## Next

1. Swap chains: `CreateSwapChain`/`Present` on an offscreen texture when there is no window, so a
   client boots headless and every frame can be hashed.
2. `ReadTexturePixels` (device and async context) through `Texture::read_back`.
3. Vertex/index buffers (`MeshGlue`) and `RenderTools.SetRenderState` + `Draw*` on the placeholder
   program; then `DrawModel` from `CModel` meshes.
4. `ISceneView`/`ISceneLayer`/`CCameraRenderer` view execution as above, then the corpus drawn.
