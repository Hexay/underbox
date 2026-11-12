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
adapter's limit). Also on the GPU: `clutter_cull_cs`, `ddgi_depth_copy_cs`,
`envmap_filtering_cs` (8×8×6 groups: the device takes the adapter's workgroup limits) and
`screen_space_reflections_cs` and `particle_lighting_cs` (research/shaders.md has the passes).
`compute/gpu_tests.rs` lists which must build. Storage images need the
adapter's format features (`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`: lavapipe writes
`Rg32Float` read-write).

## Lighting and shadows

`render/lighting/`: `Lighting::collect` reads the view's worlds (first directional light is the
sun, the rest are binned) plus the camera's `ambientColor`; worlds without lights fall back to a
fixed sun so unlit test scenes keep their goldens. It fills `ViewLightingConfigV2`,
`DirectionalLightCB`, `ClusteredLightingConstants` (one cluster holding every light) and
`BinnedLightBufferV2` in s&box's layouts, read back from the WGSL (`shading/layout.rs`).
Falloff is packed as native does (see `blocks.rs` module docs).

Shadows come from managed's shadow mapper on both paths, as on real. A native view drives it the
way native's light binner does (`lighting/shadow.rs`, ShadowMapperCallbacks.cs): `InitForView`,
`DoDirectionalLight` for the sun, `FindOrCreateShadowMaps` per shadowed point and spot light (its
return value is the light's `ShadowMapIndex`; screen size from the view camera), `UploadToGPU`,
`SetShaderAttributes`. The mapper fits the cascades (3 at High, 2048², split ratio 0.91, spheres
shrunk 4 texels for selection, 4096 units of caster pull-back), sizes the local maps
(`GetDesiredResolution`: 512² D16 for showroom's lights, cube maps one slice per face) and asks
for each shadow view through `AddShadowView` (`scene/shadow_view.rs`). That draws at once: the
parent's casters inside the shadow frustum, depth only, back faces culled, managed's bias, every
caster of one mesh as one instanced draw of the caster program (`draw/caster.rs`,
`render::Caster`). What `SetShaderAttributes` writes binds for every draw of the view
(`Lighting::shadow_attributes`). Without managed (unit tests) nothing casts. Depth convention:
`render/depth.rs` (shadow maps are reverse Z, `depth::SHADOW`).

`BinnedLightBufferV2` is native's 156-byte scalar `GPULight` (39 words, `blocks.rs`
`gpu_light`); WGSL can't declare scalar layout, so the stand-in reads it as `uint`s. A light's
world direction is `-forward` (native `GetWorldDirection` points at the light, `scene/light.rs`).

Both paths shade linear and encode once at output (docs/renderer.md, colour model): the showroom
floor matches between them to within a unit per channel.

## Managed shadows

Managed's maps are reverse-Z: they clear to 0 and compare `GREATER_EQUAL`. Under managed's scene
renderer (`r_managed_scene`) its own `ShadowSystem` renders them with `DrawModelDrawCall`, whose
bias reaches `RasterState.depth_bias`. All three kinds cast in a boot (`MANAGED=1 tools/client-frame.sh`).
The stand-in samples the maps the same way on both paths:

Sun: the stand-in (`standin_sun_shadows.hlsl`) picks the first cascade whose sphere
(`g_DirectionalLightCascadeSpheres`, radius²) holds the point, with that cascade's bias; cascade
`n`'s map is the bindless index in `DirectionalLightCB` and binds as `g_tUnderboxSunShadow`
(0) or `g_tUnderboxSunShadow1..3` (`shadow::cascade_of`, `shadow::managed_cascade`). Filtering is
core's (`Shadows/ShadowFiltering.hlsl`) at High's filter quality 3: a 16-tap tent over 8×8 texels
shaped by the cascade's hardness, a normal offset of the kernel radius plus one texel, and a blend
into the next cascade over the outer 10% of each sphere's radius².
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
