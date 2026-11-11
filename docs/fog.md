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
- Graphics programs compile at combo 0, so `volumetric_fog.fxc` gates on `g_bVolumetricFogEnabled`
  at run time instead of `D_ENABLE_VOLUMETRIC_FOG`.

## Volumetric grid

Native only renders this (`cs_volumetric_fog.shader.meta` has no source), so the grid is ours:
`fog_volume.wgsl`, 96×56×64 froxels, marched front to back per column, each slice storing the
accumulated in-scattering (rgb) and transmittance (a). Slices are linear in view depth from
`DrawDistance / 64` to `DrawDistance`, native's slicing, so managed's SceneRenderer constants
address it too. It runs once per view, not for child views (3D skybox) or orthographic ones.

Managed's SceneRenderer (`+r_managed_scene 1`) ports the native renderer's fog to
`cs_volumetric_fog.shader`, which has no source. It binds its own `FogVolume`, a volume texture
that has no GPU side here. `g_tFogVolume` therefore always binds our grid, or opaque black (no
fog) when there is none. Without that, the white default texture washed the whole frame out.
Only scene/execute.rs renders the grid, so managed views get the black one: `corpus/fog`'s managed
golden has the gradient fog and no volume haze.

These values are our choice, since no source constrains them:

- Strength 1 is one optical depth per 256 units; a box or sphere's density is
  `Strength * (1 - edge)^FalloffExponent`, where edge is 0 at the centre and 1 at the boundary.
- `Anisotropy` scales a Henyey-Greenstein asymmetry of at most 0.6 (managed's default is 1, a
  singularity if taken literally). The phase is scaled so an isotropic medium scatters 1.
- Light per froxel: ambient × `IndirectStrength`, the sun × `FogStrength`, and up to 16 local lights
  × `FogStrength` with a radial falloff. `Scattering` scales the result.

Not done: shadowed in-scattering, the baked indirect texture (`SetParams`' second argument),
temporal history and dithering (`g_vVolFogDitherScaleBias` stays zero).

## Outdoor frame's teal edges

The teal wedges at the edges of `corpus/outdoor` aren't a fog bug. They are the camera's
`BackgroundColor` (0.33, 0.46, 0.52) showing past the 3D skybox's geometry: that scene has a
`Skybox3D` of two boxes and no `SkyBox2D`, so nothing covers those pixels. Fog applies in materials
and in the sky shader's `ApplySky`, never to a raw clear.

## Tests

`sh tools/test.sh engine2 fog` covers each family through the stand-in, and `render::fog::tests`
pins the gradient and cube constants to native's formulas. `corpus/fog` is the golden on both paths: a gradient ground fog and one volume with a box behind it and one beside it.
