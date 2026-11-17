//! A light's switches and baked-lighting index as the binner packs them, after
//! `CLightBinnerStandard` as managed mirrors it (`LightBinnerFeature`): dynamic lights first (the
//! clusters hold only those), then `LIGHTTYPE_FLAGS_BAKED` ones, then the sun's baked entry and a
//! black dummy every unused bake index points at (`ViewLightingConfigV2.BakedLightIndexMapping`).

use super::{GPU_LIGHT_WORDS, Light, Lighting};
use crate::scene::light_switches::{BAKED, Switches};
use crate::scene::{light, object};
use abi::slots::render_tools as tools;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::ffi::c_void;

/// lightbinner.hlsl's `LightFlags`.
pub mod flags {
    pub const VISIBLE: u32 = 0x1;
    pub const DIFFUSE: u32 = 0x2;
    pub const SPECULAR: u32 = 0x4;
    pub const TRANSMISSIVE: u32 = 0x8;
    pub const INDEXED: u32 = 0x100;
    pub const NO_FOG_SHADOWS: u32 = 0x200;
}

/// `ViewLightingConfigV2.BakedLightIndexMapping`'s length.
pub const BAKE_INDICES: usize = 256;

/// `FogLightingMode.DynamicNoShadows`.
const FOG_DYNAMIC_NO_SHADOWS: i32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    /// `LightFlags`.
    pub flags: u32,
    pub shape: u32,
    pub shape_size: [f32; 2],
    pub modulators: [u32; 2],
    pub baked: bool,
    pub bake_index: i32,
}

impl Default for Pack {
    fn default() -> Self {
        Self::of(&Switches::default(), 0, 0, 0)
    }
}

impl Pack {
    pub fn of(s: &Switches, light_flags: u32, shape: i64, fog_mode: i32) -> Pack {
        let on = |b: bool, f: u32| if b { f } else { 0 };
        let flags = flags::VISIBLE
            | on(s.diffuse, flags::DIFFUSE)
            | on(s.specular, flags::SPECULAR)
            | on(s.transmissive, flags::TRANSMISSIVE)
            | on(s.bake_index() >= 0, flags::INDEXED)
            | on(fog_mode == FOG_DYNAMIC_NO_SHADOWS, flags::NO_FOG_SHADOWS);
        let shape = u32::try_from(shape).unwrap_or(0);
        Pack { flags, shape, shape_size: s.source_dim, modulators: s.modulators, baked: light_flags & BAKED != 0, bake_index: s.bake_index() }
    }

    pub fn from_scene(l: &light::Light) -> Pack {
        Pack::of(&l.switches, l.flags, l.shape, l.fog_mode)
    }
}

impl Lighting {
    /// Lights the clusters hold: the first ones.
    pub fn dynamic_count(&self) -> usize {
        self.lights.iter().take_while(|l| !l.pack.baked).count()
    }

    pub fn baked_count(&self) -> usize {
        self.lights.len() - self.dynamic_count()
    }

    /// `lights` with every baked one after the dynamic ones, each group in collection order.
    pub(super) fn order_baked_last(&mut self) {
        self.lights.sort_by_key(|l| l.pack.baked);
    }

    fn baked_sun(&self) -> Option<&Light> {
        self.sun.as_ref().filter(|s| (0..BAKE_INDICES as i32).contains(&s.pack.bake_index))
    }

    /// `BinnedLightBufferV2`: the lights, the sun's baked entry, then the dummy.
    pub fn binned(&self) -> Vec<[u32; GPU_LIGHT_WORDS]> {
        let mut out: Vec<_> = self.lights.iter().chain(self.baked_sun()).map(Light::gpu_light).collect();
        out.push([0; GPU_LIGHT_WORDS]);
        out
    }

    /// Each bake index's slot in [`Lighting::binned`]; the dummy's for unused ones.
    pub fn bake_mapping(&self) -> [i32; BAKE_INDICES] {
        let sun = self.baked_sun().map(|s| (s.pack.bake_index, self.lights.len()));
        let dummy = self.lights.len() + usize::from(sun.is_some());
        let mut out = [dummy as i32; BAKE_INDICES];
        let lights = self.lights.iter().enumerate().map(|(i, l)| (l.pack.bake_index, i));
        for (index, slot) in lights.chain(sun) {
            if let Some(m) = usize::try_from(index).ok().and_then(|i| out.get_mut(i)) {
                *m = slot as i32;
            }
        }
        out
    }
}

/// `RenderTools.PackSceneLight`: the light as one `GPULight` (no shadow map), its bake index, and
/// its `LIGHTTYPE_FLAGS` as the result; -1 without a light.
fn pack_scene_light(r: NativeRef<class::CSceneLightObject>, out: RawPtr<c_void>, bake_index: RawPtr<c_void>) -> i32 {
    let Some(obj) = object::get(r.cast()) else { return -1 };
    let p = obj.state.lock().unwrap().transform.position.vec;
    let Some((light, light_flags)) = obj.light.as_ref().map(|l| {
        let l = l.lock().unwrap();
        (super::from_scene(&l, glam::Vec3::new(p.x, p.y, p.z)), l.flags)
    }) else {
        return -1;
    };
    if !out.is_null() {
        // SAFETY: managed passes a whole `GpuLight`, whose size it checks against native's 156 bytes.
        unsafe { out.cast::<u32>().as_mut_slice(GPU_LIGHT_WORDS) }.copy_from_slice(&light.gpu_light());
    }
    if !bake_index.is_null() {
        // SAFETY: managed passes the address of one int.
        unsafe { *bake_index.cast::<i32>().as_ptr() = light.pack.bake_index };
    }
    light_flags as i32
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::PACK_SCENE_LIGHT, pack_scene_light);
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
