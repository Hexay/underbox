# Windows and input (SDL)

Split from docs/renderer.md.

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

## Decision: link real SDL3, not winit

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

## How it fits

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
