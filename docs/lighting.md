# Lighting, shadows and compute

Part of the renderer (docs/renderer.md).

## Compute

`RenderTools.Compute` (`render/compute/`) translates the `.shader`'s CS stage like any other
(`shading::compute`, at the static/dynamic combos the dispatch's attributes set) into a
`render::ComputeProgram` and dispatches `ceil(threads / workgroup size)` groups, bound by name
from attributes and the view's lighting. On the GPU: `clustered_light_culling_cs`,
`depthresolve_cs`, `postprocess/cmaa2_cs`, `occlusion_cull_cs` (its struct-field atomics become a
word buffer, crates/shaders `word_buffers.rs`; `compute/occlusion.rs` is the CPU fallback). Skipped
with a warning: `downsample_spd` (needs `SPV_KHR_compute_shader_derivatives`).
`compute/gpu_tests.rs` lists which must build. Storage images need the adapter's format features
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
`RasterState.depth_bias`). Managed's point and spot shadows: the stand-in reads its
`ProjectedCubeShadows`/`ProjectedShadows` structs as words (standin_managed_shadows.hlsl) and
compares against the maps they name, rendered face by face through `BindDepthTarget`'s slice.

Both paths shade linear and encode once at output (docs/renderer.md, colour model): the showroom
floor matches between them to within a unit per channel.

## Managed sun shadows (open)

Managed's cascade renders and binds, yet casts nothing in a client boot. Checked so far:
- `managed_sun_shadow_darkens_the_floor` (`draw/draw_call_tests.rs`) renders managed's shadow
  pass and main pass headless at showroom scale (`GetCascades`' 2r × 2r ortho, 4096 pull-back,
  512², bias −1 / −1.5) and passes.
- Boot probes: index 4 names `ShadowPool_512` (Depth32Float, 1 mip, 1 layer), bound to the
  stand-in with sample type Depth; after the frame it holds the boxes (1144 texels, depth
  0.348..0.382). Projecting the red box top and the floor point it shades with the CB's own
  matrix lands both on one texel: ref 0.3642 < stored 0.3698, i.e. in shadow.
- So map, matrix, index and binding agree. Suspect next: ordering — whether the shadow views'
  clear and draws reach the GPU before the main view samples the map in the same frame
  (probes read after the frame). Check the submit order of managed's shadow contexts.
