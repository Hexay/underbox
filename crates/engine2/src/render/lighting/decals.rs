//! A view's clustered decals: every rendering `CDecalSceneObject` packed as native's `GPUDecal`
//! and `GPUDecalExtraData` (common/classes/Decals.hlsl). Managed's mirror,
//! `LightBinnerFeature.Decals.cs`, documents the packing: negated position, conjugate rotation,
//! reciprocal scale, RGBE tint, sRGB colour index, extras only when one differs from its default.

use crate::render::{bindless, texture};
use crate::scene::object;
use abi::slots::c_decal_scene_object as d;
use abi::structs::Vector4;
use glam::{Quat, Vec3, Vec4};
use std::ffi::c_void;

/// Native's `kMaxDecals`.
pub const MAX: usize = 1024;
/// `GPUDecal`'s words (64 bytes).
const WORDS: usize = 16;
/// `GPUDecalExtraData`'s words.
const EXTRA_WORDS: usize = 13;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decals {
    pub count: usize,
    /// `DecalsBuffer`, sorted by sort order (stable).
    pub gpu: Vec<u32>,
    /// `DecalsExtraDataBuffer`.
    pub extra: Vec<u32>,
}

/// What a decal object holds; the fields managed's `DecalSceneObject` writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decal {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub tint: Vec4,
    pub color: i32,
    pub normal: i32,
    pub rmo: i32,
    pub height: i32,
    pub emission: i32,
    pub sort_order: u32,
    pub exclusion: u32,
    pub attenuation: f32,
    pub color_mix: f32,
    pub blend_mode: u32,
    pub parallax: f32,
    pub sampler: i32,
    pub emission_energy: f32,
    pub coverage_amount: f32,
    pub coverage_range: f32,
    pub sequence: u32,
    /// The colour texture's sheet data (`GetSheetSequenceTextureData`); zero for none.
    pub sheet: Vec4,
}

macro_rules! field {
    ($ty:ty, $slot:expr, $this:expr) => {
        abi::rt::fields::get::<$ty, { $slot.index }>($this)
    };
}

impl Decal {
    /// The decal at `this`, as its fields, attributes and scene object state hold it.
    pub fn read(this: *mut c_void, obj: &object::SceneObject) -> Decal {
        let t = obj.state.lock().unwrap().transform;
        let (p, s, q) = (t.position.vec, t.scale.vec, t.rotation.quat);
        let rotation = Quat::from_xyzw(q.x, q.y, q.z, q.w);
        let tint: Vector4 = field!(Vector4, d::GET_M_V_COLOR_TINT, this);
        let color: texture::Ref = field!(texture::Ref, d::GET_M_H_COLOR, this);
        Decal {
            position: Vec3::new(p.x, p.y, p.z),
            rotation: if rotation.length_squared() > 0.0 { rotation.normalize() } else { Quat::IDENTITY },
            scale: Vec3::new(s.x, s.y, s.z),
            tint: Vec4::new(tint.vec.x, tint.vec.y, tint.vec.z, tint.vec.w),
            color: bindless::index(color, true),
            normal: bindless::index(field!(texture::Ref, d::GET_M_H_NORMAL, this), false),
            rmo: bindless::index(field!(texture::Ref, d::GET_M_H_RMO, this), false),
            height: bindless::index(field!(texture::Ref, d::GET_M_H_HEIGHT, this), false),
            emission: bindless::index(field!(texture::Ref, d::GET_M_H_EMISSION, this), false),
            sort_order: field!(u32, d::GET_M_N_SORT_ORDER, this),
            exclusion: field!(u32, d::GET_M_N_EXCLUSION_BIT_MASK, this),
            attenuation: field!(f32, d::GET_M_FL_ATTENUATION_ANGLE, this),
            color_mix: field!(f32, d::GET_M_FL_COLOR_MIX, this),
            blend_mode: crate::render::attributes::shader_value(obj.attributes, "DecalBlendMode").map_or(0, |v| v.x as u32),
            parallax: field!(f32, d::GET_M_FL_PARALLAX_STRENGTH, this),
            sampler: field!(i32, d::GET_M_N_SAMPLER_INDEX, this),
            emission_energy: field!(f32, d::GET_M_FL_EMISSION_ENERGY, this),
            coverage_amount: field!(f32, d::GET_M_FL_COVERAGE_AMOUNT, this),
            coverage_range: field!(f32, d::GET_M_FL_COVERAGE_RANGE, this),
            sequence: field!(u32, d::GET_M_N_SEQUENCE_INDEX, this),
            // TODO: sheeted decal textures (`GetSheetSequenceTextureData` isn't implemented).
            sheet: Vec4::ZERO,
        }
    }

    /// The decal's `GPUDecal` words and its extras (`None` when every extra is the default).
    fn pack(&self) -> ([u32; WORDS], Option<[u32; EXTRA_WORDS]>) {
        let (p, q, s) = (-self.position, self.rotation, self.scale.recip());
        let (rgb, exp) = rgbe(self.tint.truncate());
        let alpha = (self.tint.w * 255.0) as u8 as u32;
        let attenuation = (self.attenuation * 255.0 + 0.5) as u8 as u64;
        let packed = (self.color as u64 & 0xFFFF) | (self.normal as u64 & 0xFFFF) << 16 | (self.rmo as u64 & 0xFFFF) << 32 | (exp as u64) << 48 | attenuation << 56;
        let mut w = [0u32; WORDS];
        w[..10].copy_from_slice(&[p.x, p.y, p.z, -q.x, -q.y, -q.z, q.w, s.x, s.y, s.z].map(f32::to_bits));
        w[10..16].copy_from_slice(&[packed as u32, (packed >> 32) as u32, self.sort_order, self.exclusion, rgb | alpha << 24, u32::MAX]);

        let mut has_extra = self.blend_mode != 0;
        let (mut parallax, mut coverage, mut energy, mut sequence) = (1.0f32, 0xFFu32, 1.0f32, 0xFF_FFFFu32);
        if self.height > 0 {
            parallax = self.parallax;
            let byte = |v: f32, hi: f32| (v.clamp(0.0, hi) * 255.0 + 0.5) as u8 as u32;
            coverage = byte(self.coverage_amount, 1.0) | byte(self.coverage_range, 0.5) << 8;
            has_extra = true;
        }
        if self.emission > 0 {
            energy = self.emission_energy.max(0.0);
            has_extra = true;
        }
        let (mut sheet, mut color_mix, mut sampler) = (Vec4::ZERO, 1.0f32, -1i32);
        if self.color > 0 {
            if self.sheet != Vec4::ZERO {
                (sheet, sequence) = (self.sheet, self.sequence);
                has_extra = true;
            }
            if self.color_mix < 1.0 {
                color_mix = self.color_mix;
                has_extra = true;
            }
            if self.sampler >= 0 {
                sampler = self.sampler;
                has_extra = true;
            }
        }
        let extra = has_extra.then(|| {
            let f = f32::to_bits;
            [f(sheet.x), f(sheet.y), f(sheet.z), f(sheet.w), sequence, f(color_mix), self.blend_mode, self.height as u32, f(parallax), sampler as u32, self.emission as u32, f(energy), coverage]
        });
        (w, extra)
    }
}

/// Managed's `Color.ToRgbe`: signed 8-bit mantissas biased by 128 and a shared exponent biased by 128.
fn rgbe(c: Vec3) -> (u32, u8) {
    let max = c.abs().max_element();
    if max < 1e-16 {
        return (0, 0);
    }
    let exp = ((max.to_bits() >> 23) & 0xFF) as i32 - 126;
    let scale = f32::from_bits(((134 - exp) as u32) << 23);
    let m = |v: f32| ((v * scale + 0.5) as i32).clamp(-128, 127) + 128;
    (m(c.x) as u32 | (m(c.y) as u32) << 8 | (m(c.z) as u32) << 16, (exp.clamp(-128, 127) + 128) as u8)
}

impl Decals {
    /// Pack `decals` in sort order, keeping the given order among equals.
    pub fn pack(mut decals: Vec<Decal>) -> Decals {
        decals.truncate(MAX);
        decals.sort_by_key(|d| d.sort_order);
        let mut out = Decals { count: decals.len(), ..Default::default() };
        for decal in &decals {
            let (mut w, extra) = decal.pack();
            if let Some(extra) = extra {
                w[15] = (out.extra.len() * 4) as u32;
                out.extra.extend_from_slice(&extra);
            }
            out.gpu.extend_from_slice(&w);
        }
        out
    }

    pub fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

#[cfg(test)]
#[path = "decals_tests.rs"]
mod tests;
