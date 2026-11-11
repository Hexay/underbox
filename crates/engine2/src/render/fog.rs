//! The per-view fog constants of [`BLOCK`] (vr_common.fxc): managed's gradient and cubemap fog
//! attributes in the form core's Fog.hlsl reads, and the froxel mapping of render/fog_volume.rs.
//! Analytic fog is managed's own `FogConstants` block. See docs/fog.md.

use crate::render::lighting::Lighting;
use crate::render::shading::layout::Value;
use crate::render::{attributes, scene_utils, texture};
use abi::structs::SceneVolumetricFogParameters;
use abi::{NativeRef, StringToken};
use glam::{Mat4, Vec4};

/// The constant block these live in, as DXC names the cbuffer.
pub const BLOCK: &str = "PerViewConstantBufferVR_t";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    bias_scale: Vec4,
    exponents: Vec4,
    color_opacity: Vec4,
    culling: Vec4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cube {
    offset_scale_bias_exponent: Vec4,
    height_offset_scale_exponent_mips: Vec4,
    world_to_sky: Mat4,
    culling: Vec4,
    texture: texture::Ref,
}

/// What a camera's attributes say about its fog.
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewFog {
    pub(crate) gradient: Option<Gradient>,
    pub(crate) cube: Option<Cube>,
    pub volumetric: Option<SceneVolumetricFogParameters>,
}

impl PartialEq for ViewFog {
    fn eq(&self, other: &ViewFog) -> bool {
        (self.gradient, self.cube, self.volumetric.is_some()) == (other.gradient, other.cube, other.volumetric.is_some())
    }
}

/// `1 / (end - start)`, finite for an empty range.
fn inverse_range(start: f32, end: f32) -> f32 {
    let range = end - start;
    if range.abs() > 1e-5 { range.recip() } else { 1e5 }
}

/// Native's `SetupGradientFog`, as managed's SceneRenderer ports it (Gpu/ViewEnvironment.cs), from
/// `GradientFogSetup.Apply`'s attributes: `StartDistance, EndDistance, StartHeight, EndHeight`,
/// `MaximumOpacity, DistanceFalloffExponent, VerticalFalloffExponent` and the colour.
pub(crate) fn gradient(params: Vec4, params2: Vec4, color: Vec4) -> Gradient {
    let distance = inverse_range(params.x, params.y);
    // Full at StartHeight, none at EndHeight; everything above EndHeight is culled.
    let height = inverse_range(params.w, params.z);
    Gradient {
        bias_scale: Vec4::new(-params.x * distance, -params.w * height, distance, height),
        exponents: Vec4::new(params2.y, params2.z, 0.0, 0.0),
        color_opacity: color.truncate().extend(params2.x),
        culling: Vec4::new(params.x * params.x, params.w, 0.0, 0.0),
    }
}

/// `CubemapFogController.Write`'s attributes: `StartDistance, EndDistance, LodBias, FalloffExponent`
/// and `HeightWidth, HeightStart, HeightExponent`; `transform` is the shader's matrix, which
/// Fog.hlsl multiplies on the left (a rotation's inverse).
pub(crate) fn cube(params: Vec4, params2: Vec4, transform: Mat4, texture: texture::Ref, size: u32) -> Cube {
    let scale = inverse_range(params.x, params.y);
    let (width, start, exponent) = (params2.x, params2.y, params2.z);
    let (height_offset, height_scale, height_exponent, height_cull) = if width > 0.0 { (-start / width, width.recip(), exponent, start) } else { (0.0, 0.0, 1.0, f32::MAX) };
    Cube {
        offset_scale_bias_exponent: Vec4::new(-params.x * scale, scale, params.z, params.w),
        height_offset_scale_exponent_mips: Vec4::new(height_offset, height_scale, height_exponent, (size.max(1) as f32).log2() - 3.0),
        world_to_sky: transform,
        culling: Vec4::new(params.x * params.x, height_cull, 0.0, 0.0),
        texture,
    }
}

fn vector(attrs: attributes::Ref, name: &str) -> Vec4 {
    attributes::shader_value(attrs, name).unwrap_or(Vec4::ZERO)
}

fn gather_gradient(attrs: attributes::Ref) -> Option<Gradient> {
    (attributes::combo(attrs, "D_ENABLE_GRADIENT_FOG") == Some(1)).then(|| gradient(vector(attrs, "GradientFogParams"), vector(attrs, "GradientFogParams2"), vector(attrs, "GradientFogParams3")))
}

fn gather_cube(attrs: attributes::Ref) -> Option<Cube> {
    if attributes::combo(attrs, "D_ENABLE_CUBEMAP_FOG") != Some(1) {
        return None;
    }
    let t = attributes::texture(attrs, "CubemapFogTexture")?;
    let width = texture::resolve(t)?.gpu.as_ref()?.desc.width;
    // Native writes `Matrix.FromTransform`'s numerics transposed into a column-major cbuffer: the shader sees them as-is.
    let transform = attributes::matrix(attrs, "CubeFogTransform").map_or(Mat4::IDENTITY, crate::render::scene::camera::mat4);
    Some(cube(vector(attrs, "CubeFogParams"), vector(attrs, "CubeFogParams2"), transform, t, width))
}

fn gather_volumetric(attrs: attributes::Ref) -> Option<SceneVolumetricFogParameters> {
    let fog = attributes::pointer(attrs, StringToken::of("IVolumetricFog"))?;
    let fog = objects::get::<_, scene_utils::Fog>(NativeRef::<abi::class::IVolumetricFog>::from_raw(fog as *mut _))?;
    let (params, _baked) = (*fog.0.lock().unwrap())?;
    (params.m_b_fog_enabled != 0).then_some(params)
}

/// The fog a camera's attributes configure.
pub fn gather(attrs: attributes::Ref) -> ViewFog {
    ViewFog { gradient: gather_gradient(attrs), cube: gather_cube(attrs), volumetric: gather_volumetric(attrs) }
}

/// A per-view fog constant. With no fog set, the culling bounds still reject every surface:
/// zeroes would pass Fog.hlsl's tests and paint the cubemap's default over the frame.
pub fn constant(lighting: &Lighting, name: &str) -> Option<Value> {
    let fog = lighting.view_fog.as_deref();
    let gradient = fog.and_then(|f| f.gradient.as_ref());
    let cube = fog.and_then(|f| f.cube.as_ref());
    let frustum = lighting.fog_volume.as_deref();
    let vec = |v: Option<Vec4>| v.map(Value::Vec);
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    match name {
        "g_bFogTypeEnabled" => vec(Some(Vec4::new(flag(frustum.is_some()), flag(gradient.is_some()), flag(cube.is_some()), 0.0))),
        "g_vGradientFogBiasAndScale" => vec(gradient.map(|g| g.bias_scale)),
        "m_vGradientFogExponents" => vec(gradient.map(|g| g.exponents)),
        "g_vGradientFogColor_Opacity" => vec(gradient.map(|g| g.color_opacity)),
        "g_vGradientFogCullingParams" => vec(Some(gradient.map_or(Vec4::new(f32::MAX, f32::MIN, 0.0, 0.0), |g| g.culling))),
        "g_vCubeFog_Offset_Scale_Bias_Exponent" => vec(cube.map(|c| c.offset_scale_bias_exponent)),
        "g_vCubeFog_Height_Offset_Scale_Exponent_Log2Mip" => vec(cube.map(|c| c.height_offset_scale_exponent_mips)),
        "g_matvCubeFogSkyWsToOs" => cube.map(|c| Value::Mat(c.world_to_sky)),
        "g_vCubeFogCullingParams" => vec(Some(cube.map_or(Vec4::new(f32::MAX, f32::MAX, 0.0, 0.0), |c| c.culling))),
        "g_vVolFog_VolumeScale_Shift_Near_Range" => vec(frustum.map(|f| Vec4::new(1.0, 0.0, f.near, f.range))),
        "g_vVolFogPostWorldToFrustumScale" => vec(frustum.map(|f| Vec4::new(0.5, 0.5, f.range.recip(), 0.0))),
        "g_vVolFogPostWorldToFrustumBias" => vec(frustum.map(|f| Vec4::new(0.5, 0.5, -f.near / f.range, 0.0))),
        "g_mVolFogFromWorld" => frustum.map(|f| Value::Array(vec![Value::Mat(f.world_to_clip), Value::Mat(f.world_to_clip)])),
        _ => None,
    }
}

/// `FogSunColor` / `FogSunDirection` from the view's sun, for views managed's shadow mapper
/// doesn't set them on. Native's sun direction points at the light (ShadowMapper.Native.cs).
pub fn sun_value(lighting: &Lighting, name: &str) -> Option<Vec4> {
    let sun = lighting.sun.as_ref()?;
    match name {
        "FogSunColor" => Some(sun.color.extend(0.0)),
        "FogSunDirection" => Some((-sun.forward()).extend(0.0)),
        _ => None,
    }
}

/// The cubemap fog's texture, for its binding.
pub fn cube_texture(lighting: &Lighting) -> Option<texture::Ref> {
    lighting.view_fog.as_ref()?.cube.map(|c| c.texture)
}

/// The numbers managed SceneRenderer's port of native (`SetupGradientFog`, `SetupCubemapFog`)
/// computes for the same settings.
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec4, b: Vec4) -> bool {
        (a - b).abs().max_element() < 1e-6
    }

    #[test]
    fn gradient_constants_match_native() {
        // StartDistance 100, EndDistance 1100, StartHeight 0, EndHeight 60.
        let g = gradient(Vec4::new(100.0, 1100.0, 0.0, 60.0), Vec4::new(0.8, 2.0, 3.0, 0.0), Vec4::new(0.5, 0.25, 1.0, 0.0));
        assert!(close(g.bias_scale, Vec4::new(-0.1, 1.0, 0.001, -1.0 / 60.0)), "{:?}", g.bias_scale);
        assert_eq!((g.exponents, g.color_opacity, g.culling), (Vec4::new(2.0, 3.0, 0.0, 0.0), Vec4::new(0.5, 0.25, 1.0, 0.8), Vec4::new(10_000.0, 60.0, 0.0, 0.0)));
    }

    #[test]
    fn cube_constants_match_native() {
        let t = texture::Ref::default();
        let c = cube(Vec4::new(10.0, 4106.0, 0.5, 2.0), Vec4::new(100.0, 2000.0, 2.0, 0.0), Mat4::IDENTITY, t, 256);
        assert!(close(c.offset_scale_bias_exponent, Vec4::new(-10.0 / 4096.0, 1.0 / 4096.0, 0.5, 2.0)), "{:?}", c.offset_scale_bias_exponent);
        assert!(close(c.height_offset_scale_exponent_mips, Vec4::new(-20.0, 0.01, 2.0, 5.0)), "{:?}", c.height_offset_scale_exponent_mips);
        assert_eq!(c.culling, Vec4::new(100.0, 2000.0, 0.0, 0.0));
        let flat = cube(Vec4::new(10.0, 4106.0, 0.5, 2.0), Vec4::new(0.0, 2000.0, 2.0, 0.0), Mat4::IDENTITY, t, 256);
        assert_eq!((flat.height_offset_scale_exponent_mips, flat.culling.y), (Vec4::new(0.0, 0.0, 1.0, 5.0), f32::MAX));
    }
}
