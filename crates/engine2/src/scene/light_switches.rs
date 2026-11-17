//! A light's render switches and baked-lighting index: what `CSceneLightObject`'s setters store and
//! the light binner packs (render/lighting/pack.rs).

/// `LIGHTTYPE_FLAGS_MIXED_SHADOWS` (native's lightdesc.h, managed's `SceneMirror`).
pub const MIXED_SHADOWS: u32 = 16;
/// `LIGHTTYPE_FLAGS_BAKED`: binned after the dynamic lights, outside the clusters.
pub const BAKED: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Switches {
    pub diffuse: bool,
    pub specular: bool,
    pub transmissive: bool,
    /// `SetLightModulatorRange`: offset and count in the modulator buffer.
    pub modulators: [u32; 2],
    pub bake_index: i32,
    pub bake_index_scale: f32,
    pub indexed: bool,
    pub source_size: [f32; 2],
    pub source_dim: [f32; 2],
}

impl Default for Switches {
    fn default() -> Self {
        Self { diffuse: true, specular: true, transmissive: true, modulators: [0; 2], bake_index: -1, bake_index_scale: 1.0, indexed: false, source_size: [0.0; 2], source_dim: [0.0; 2] }
    }
}

impl Switches {
    /// The index lightmaps and probes address this light by, or -1 (`PackSceneLight`'s out index).
    pub fn bake_index(&self) -> i32 {
        if self.indexed && (0..=254).contains(&self.bake_index) { self.bake_index } else { -1 }
    }
}
