# Lighting, shadows and compute

Part of the renderer (docs/renderer.md).

## Compute

`RenderTools.Compute` (`render/compute/`) translates the `.shader`'s CS stage like any other
(`shading::compute`, at the static/dynamic combos the dispatch's attributes set) into a
`render::ComputeProgram` and dispatches `ceil(threads / workgroup size)` groups, bound by name
from attributes and the view's lighting. All on the GPU: `clustered_light_culling_cs`,
`depthresolve_cs`, `postprocess/cmaa2_cs`, `occlusion_cull_cs` (its struct-field atomics become a
word buffer, crates/shaders `word_buffers.rs`; `compute/occlusion.rs` is the CPU fallback) and
`downsample_spd` (`spirv::strip_compute_derivatives` drops the extension DXC adds for its quad
ops; each `RWTexture2D` binds the mip its attribute names; seven storage images need the
adapter's limit). `compute/gpu_tests.rs` lists which must build. Storage images need the
adapter's format features (`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`: lavapipe writes
`Rg32Float` read-write).

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

Both paths shade linear and encode once at output (docs/renderer.md, colour model): the showroom
floor matches between them to within a unit per channel.

## Managed shadows

Managed's scene renderer (`r_managed_scene`) is reverse-Z: its maps clear to 0 and compare
`GREATER_EQUAL`; the stand-in tells its sun cascades from ours by `ShadowMapIndex` (ours is the
sentinel `0x7FFFFF00`). Shadow draws carry managed's rasterizer bias (`DrawModelDrawCall` slots →
`RasterState.depth_bias`). All three kinds cast in a boot (`MANAGED=1 tools/client-frame.sh`).

Sun: the stand-in (`standin_sun_shadows.hlsl`) picks the first cascade whose sphere
(`g_DirectionalLightCascadeSpheres`, radius²) holds the point, with that cascade's bias; cascade
`n`'s map is the bindless index in `DirectionalLightCB` and binds as `g_tUnderboxSunShadow`
(0) or `g_tUnderboxSunShadow1..3` (`shadow::cascade_of`, `shadow::managed_cascade`).
`draw/managed_shadow_tests.rs` replays the boot's order headless with two cascades.

Point and spot: the stand-in reads `ProjectedCubeShadows`/`ProjectedShadows` as words
(`standin_managed_shadows.hlsl`) and compares against the maps they name, rendered face by face
through `BindDepthTarget`'s slice. naga can't compare-sample a binding array, so the maps come
through fixed slots (`shading/shadow_slots.rs`).

Shadow quality: `RenderSettings.DetectPreset` reads `SystemInfo.SetGpu`, which `SourceEngineInit`
reports (`device::report_to_managed`, memory from `Gpu::device_local_bytes`). Without it the
preset was Low: one 512² cascade over 4000 units, ~3 texels per showroom shadow. `client-frame.sh`
pins `shadow.quality` to High (`SHADOW_QUALITY`, `tools/video-setting.py`: three cascades, 2048²)
so goldens don't depend on the detected preset.

Debugging: `UNDERBOX_SUBMITS=log` prints every submit with its PIX scopes, target and view (setup
compute, depth prepass, cull, shadow cascades then local maps, opaque, post, output).
`UNDERBOX_STANDIN_DEBUG=1` paints sun visibility, `2` cascade 0's shadow-space position.
