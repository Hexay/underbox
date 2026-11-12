# Fog

Every fog family reaches a surface through core's `common/classes/Fog.hlsl`. The stand-ins include
that file rather than porting it, so closed shaders and shaders with source shade alike.

## Inputs

| Family | Managed sets | We fill |
|---|---|---|
| Analytic (`Sandbox.Fog`), sky tint, sun lobe | `FogConstants` / `SkyboxFogConstants` block | the block as given; the tint cube through its bindless index |
| Sun lobe light | `FogSunColor`, `FogSunDirection` on the view (`ShadowMapper.Native.cs`) | the view's sun where those aren't set (`fog::sun_value`) |
| Gradient | `GradientFogParams`, `2`, `3`, combo `D_ENABLE_GRADIENT_FOG` | vr_common's `g_vGradientFog*` (`fog::gradient`) |
| Cubemap | `CubeFogParams`, `2`, `CubeFogTransform`, `CubemapFogColor`, `CubemapFogTexture`, combo `D_ENABLE_CUBEMAP_FOG` | `g_vCubeFog*`, `g_matvCubeFogSkyWsToOs` (`fog::cube`) |
| Volumetric | `IVolumetricFog` pointer attribute, `SetParams`, `AddVolumetricFogVolume` | a froxel grid (`fog_volume.rs`) and `g_mVolFogFromWorld` |

- Native computes vr_common's gradient and cube constants in closed code; `fog::gradient` and
  `fog::cube` follow managed SceneRenderer's port of it (`Sandbox.SceneRenderer/Gpu/ViewEnvironment.cs`).
  That path fills `PerViewConstantBufferVR_t` itself, so our values only apply where it doesn't.
- With a family off, its culling bounds still have to reject everything: all-zero constants pass
  Fog.hlsl's tests and paint the default cubemap over the frame.
- `g_matViewToProjection` carries only `[3].w`, which Fog.hlsl reads to tell orthographic views
  from perspective ones.
- `volumetric_fog.fxc` also gates on `g_bVolumetricFogEnabled` at run time, so a draw that never
  sets `D_ENABLE_VOLUMETRIC_FOG` (programs build at the combos the draw's attributes set) still
  gets the grid.

## Volumetric grid

Native only renders this (`cs_volumetric_fog.shader.meta` has no source), so the grid is ours:
`fog_volume.wgsl`, 96×56×64 froxels, marched front to back per column, each slice storing the
accumulated in-scattering (rgb) and transmittance (a). Slices are linear in view depth from
`DrawDistance / 64` to `DrawDistance`, native's slicing, so managed's SceneRenderer constants
address it too. It runs once per view, not for child views (3D skybox) or orthographic ones,
and not while `r_enable_volume_fog` is 0: the quality profile's Off level (the Low preset) sets
that native convar (`render/convars.rs`, announced at `SourceEngineInit`).

Managed's SceneRenderer (`+r_managed_scene 1`) ports the native renderer's fog to
`cs_volumetric_fog.shader`, which has no source, writing its own `FogVolume` volume texture (no GPU
side here). `render/fog_managed.rs` stands in for that dispatch: it rebuilds the view, volumes and
settings from `VolumetricFogConstantBuffer_t`, the sun from `DirectionalLightCB` and ambient from
`ViewLightingConfigV2`, marches the same grid, and keys it by managed's `OutScatteringTexture`.
Where shading binds that texture as `FogVolume`, `g_tFogVolume` gets the grid. Otherwise it binds
the view's own grid, or opaque black (no fog); the white default texture would wash the frame out.

- The rebuilt world-to-clip spans the froxel corner rays with w as view depth, which is what
  managed's `g_mVolFogFromWorld` gives shading; its z only keeps it invertible.
- Managed scales `Scattering` by 0.0022 for its shader; the stand-in undoes that.
- Local lights come from managed's `BinnedLightBufferV2`, read back from the GPU: the first
  `NumLights.x + .y` (`ViewLightingConfigV2`) 156-byte `GpuLight`s, each colour × its fog
  strength. A light with a cone (word 29 non-zero) is a spot (`fog_managed::lights_from`).

These values are our choice, since no source constrains them:

- Strength 1 is one optical depth per 256 units; a box or sphere's density is
  `Strength * (1 - edge)^FalloffExponent`, where edge is 0 at the centre and 1 at the boundary.
- `Anisotropy` scales a Henyey-Greenstein asymmetry of at most 0.6 (managed's default is 1, a
  singularity if taken literally). The phase is scaled so an isotropic medium scatters 1.
- Light per froxel: ambient × `IndirectStrength`, the sun × `FogStrength`, and up to 16 local lights
  × `FogStrength` with a radial falloff; a spot also fades smoothly from its inner to its outer
  cone. `Scattering` scales the result.
- The sun's term is shadowed: a 4x4-texel tent (bilinear-weighted 3x3 PCF) of its shadow map per
  froxel, with a 1e-3 depth bias; points outside the map are lit. A single tap banded at the
  shadow's edge. The native path uses ours (`lighting/shadow.rs`); the managed path uses managed's
  cascade 0 only (`shadow::managed_sun_shadow`: reverse-Z, world → uv in `DirectionalLightCB`
  words 8..24), so fog past cascade 0 is unshadowed.
- Temporal blend: each froxel's lighting (in-scattered rgb, density) is 0.3 this frame and 0.7
  the nearest froxel of last frame's grid at the same world point, reprojected through last
  frame's view. Points outside last frame's grid take this frame's alone. History is kept per
  target (the view's colour target, or managed's `OutScatteringTexture`) in `fog_march.rs`.
  History within 1/256 of this frame's value is replaced by it: the history is half precision,
  and a rounded 0.3/0.7 blend stalls anywhere within about 1.7 ulps of its target. Without the
  snap a still view settled on a value set by the frames before it, which vary with machine
  load, and the fog golden flipped 1 LSB between runs.

Not done: managed cascades past 0, the baked indirect texture (`SetParams`'
second argument), local lights' shadows, and dithering (`g_vVolFogDitherScaleBias` stays zero).

## Outdoor frame's teal edges

The teal wedges at the edges of `corpus/outdoor` aren't a fog bug. They are the camera's
`BackgroundColor` (0.33, 0.46, 0.52) showing past the 3D skybox's geometry: that scene has a
`Skybox3D` of two boxes and no `SkyBox2D`, so nothing covers those pixels. Fog applies in materials
and in the sky shader's `ApplySky`, never to a raw clear.

## Tests

`sh tools/test.sh engine2 fog` covers each family through the stand-in, `render::fog::tests`
pins the gradient and cube constants to native's formulas, and `fog_managed_tests` reads managed's
constants back. `corpus/fog` is the golden on both paths: a gradient ground fog and one volume with a box behind it and one beside it, under a
shadow-casting sun and an off-screen roof whose shadow crosses the volume.
