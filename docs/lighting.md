# Lighting, shadows and compute

Part of the renderer (docs/renderer.md).

## Compute

`RenderTools.Compute` (`render/compute/`) translates the `.shader`'s CS stage like any other
(`shading::compute`, combos 0/0) into a `render::ComputeProgram` and dispatches
`ceil(threads / workgroup size)` groups, bound by name from attributes and the view's lighting.
On the GPU: `clustered_light_culling_cs`, `depthresolve_cs`, `postprocess/cmaa2_cs`. On the CPU:
`occlusion_cull_cs` (naga rejects DXC's output; `render/compute/` keeps a CPU cull). Skipped with a
warning: `downsample_spd` (needs `SPV_KHR_compute_shader_derivatives`). `compute/gpu_tests.rs`
lists which must build. Storage images need the adapter's format features
(`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`: lavapipe writes `Rg32Float` read-write).

## Lighting and shadows

`render/lighting/`: `Lighting::collect` reads the view's worlds (first directional light is the
sun, the rest are binned) plus the camera's `ambientColor`; worlds without lights fall back to a
fixed sun so unlit test scenes keep their goldens. It fills `ViewLightingConfigV2`,
`DirectionalLightCB`, `ClusteredLightingConstants` (one cluster holding every light) and
`BinnedLightBufferV2` in s&box's layouts, read back from the WGSL (`shading/layout.rs`).
Falloff is packed as native does (see `blocks.rs` module docs). Sun shadows: one 2048² depth map
fitted to the view frustum and texel-snapped (`lighting/shadow.rs`), sampled by the stand-in with
3×3 PCF as `g_tUnderboxSunShadow`. Point and spot shadows: a 4×4 atlas of 512² tiles, six per
point light and one per spot (`lighting/local_shadow.rs`), named by sentinel `ShadowMapIndex`.

`BinnedLightBufferV2` is native's 156-byte scalar `GPULight` (39 words, `blocks.rs`
`gpu_light`); WGSL can't declare scalar layout, so the stand-in reads it as `uint`s. A light's
world direction is `-forward` (native `GetWorldDirection` points at the light, `scene/light.rs`).

Managed's scene renderer (`r_managed_scene`) is reverse-Z: its cascade maps clear to 0 and compare
`GREATER_EQUAL`; the stand-in tells them from ours by `ShadowMapIndex` (ours is the sentinel
`0x7FFFFF00`). Shadow draws carry managed's rasterizer bias (`DrawModelDrawCall` slots →
`RasterState.depth_bias`).

Tone gap (open): managed's showroom floor (136,153,120) is exactly native's (63,81,48)
sRGB-encoded. Cause: color textures are sampled without their `SrgbRead( true )` decode — the
binder ignores the annotation and textures have no sRGB view (`render::Texture`, empty
`view_formats`). Native writes the undecoded value straight to the 8-bit target, so the two
errors look right; managed lights in linear HDR and encodes at `ResolveHDRToFinalSDR`, so the
texture comes out encoded once too often. Encoding native's blit alone washed everything out for
the same reason. Fix: sRGB views for `SrgbRead` textures, then the encode on native's output.

## Managed sun shadows

Submit order was fine (`UNDERBOX_SUBMITS=log` prints every submit with its PIX scopes, target and
view: setup compute, depth prepass, cull, shadow cascades then local maps, opaque, post, output).
The cause was quality: the engine never called `SystemInfo.SetGpu`, so `RenderSettings.DetectPreset`
saw no VRAM and stored Low in `config/video.json` — one 512² cascade over 4000 units, ~3 texels
per showroom shadow. `SourceEngineInit` now reports the adapter (`device::report_to_managed`;
memory is still 0, so this machine still detects Low), and `client-frame.sh` pins
`shadow.quality` to High (`SHADOW_QUALITY`, `tools/video-setting.py`): three cascades, 2048².

The stand-in picks the first cascade whose sphere (`g_DirectionalLightCascadeSpheres`, radius²)
holds the point, with that cascade's bias; cascades 1–3 bind as `g_tUnderboxSunShadow1..3`
(`shadow::cascade_of`). `draw/managed_shadow_tests.rs` replays the boot's order headless with two
cascades. `UNDERBOX_STANDIN_DEBUG=1` paints sun visibility, `2` cascade 0's shadow-space position.
