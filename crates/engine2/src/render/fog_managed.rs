//! Stand-in for managed SceneRenderer's `cs_volumetric_fog` (it ships without source): its
//! dispatch marches our froxel grid (fog_volume.rs) from the `VolumetricFogConstantBuffer_t`
//! managed fills (`Pipeline/Fog/VolumetricFog.cs`), keyed by the `OutScatteringTexture` it would
//! write. Shading binds that grid wherever managed binds the texture as `FogVolume`. docs/fog.md.

use crate::render::fog_volume::{self, FogLight, Frustum, Settings, Sources, Volume};
use crate::render::{attributes, buffers, device};
use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

pub const SHADER: &str = "cs_volumetric_fog";
pub const CONSTANTS: &str = "VolumetricFogConstantBuffer_t";
const LIGHTS: &str = "BinnedLightBufferV2";
const LIGHT_BYTES: usize = 156;
const MAX_VOLUMES: usize = 128;
/// Managed hands the shader `Scattering` × this (`VolumetricFog.BeginFrame`); ours takes it raw.
const SCATTERING_SCALE: f32 = 0.0022;

/// Word offsets in [`CONSTANTS`] (sequential C# layout, 16-word matrices).
mod at {
    pub const VIEW_DX: usize = 0;
    pub const VIEW_DY: usize = 4;
    #[cfg(test)]
    pub const VIEW_CORNER: usize = 8;
    pub const VIEW_POS: usize = 12;
    pub const VIEW_DIR: usize = 16;
    pub const NEAR_FAR: usize = 36;
    pub const ANISOTROPY_SCATTERING: usize = 44;
    pub const INDIRECT_STRENGTH: usize = 51;
    pub const FADE_IN_SCALE_BIAS: usize = 72;
    pub const VOLUME_COUNT: usize = 76;
    pub const BOX_MIN: usize = 80;
    pub const BOX_MAX: usize = BOX_MIN + 4 * super::MAX_VOLUMES;
    pub const BOX_COLOR_EXPONENT: usize = BOX_MAX + 4 * super::MAX_VOLUMES;
    pub const BOX_WORLD_TO_VOLUME: usize = BOX_COLOR_EXPONENT + 4 * super::MAX_VOLUMES;
    pub const WIDTH_HEIGHT_DEPTH: usize = BOX_WORLD_TO_VOLUME + 16 * super::MAX_VOLUMES;
}

static GRIDS: LazyLock<Mutex<HashMap<usize, Arc<Frustum>>>> = LazyLock::new(Default::default);

/// The grid marched for managed's fog texture at `texture` (an address), if any.
pub fn grid(texture: usize) -> Option<Arc<Frustum>> {
    GRIDS.lock().unwrap().get(&texture).cloned()
}

/// One `cs_volumetric_fog` dispatch. Managed runs a lighting pass then an integrate pass
/// (`D_PASS` 0, 1); one march stands in for both, on pass 0.
pub fn dispatch(attrs: &[attributes::Ref]) -> Result<(), String> {
    if attrs.iter().find_map(|a| attributes::combo(*a, "D_PASS")).unwrap_or(0) != 0 {
        return Ok(());
    }
    let block = |name: &str| attrs.iter().find_map(|a| attributes::constant_buffer(*a, name));
    let out = attrs.iter().find_map(|a| attributes::texture(*a, "OutScatteringTexture")).ok_or("no OutScatteringTexture")?;
    let fog = Constants(block(CONSTANTS).ok_or("no VolumetricFogConstantBuffer_t")?.to_vec());
    let config = block("ViewLightingConfigV2").map(|b| words(&b)).unwrap_or_default();
    let ambient = vec3(&config, 1028);
    // NumLights: dynamic, then baked indexed lights.
    let count = config.iter().take(2).map(|n| n.to_bits() as usize).sum();
    let lights = attrs.iter().find_map(|a| attributes::buffer(*a, LIGHTS)).map_or_else(Vec::new, |h| binned_lights(h, count));
    let sources = Sources { ambient: ambient * fog.f(at::INDIRECT_STRENGTH), sun: block("DirectionalLightCB").and_then(|b| sun(&b)), lights };
    let grid = fog_volume::march(&fog.volumes(), fog.view_proj()?, &sources, &fog.settings(), out.addr());
    let mut grids = GRIDS.lock().unwrap();
    match grid {
        Some(g) => grids.insert(out.addr(), g),
        None => grids.remove(&out.addr()),
    };
    Ok(())
}

/// The first `count` of managed's binned lights, read back from the GPU.
fn binned_lights(handle: usize, count: usize) -> Vec<FogLight> {
    let (Some(gpu), Some(buffer)) = (device::gpu(), buffers::get(buffers::Handle::new(handle))) else { return Vec::new() };
    let len = (count as u64 * LIGHT_BYTES as u64).min(buffer.size() / LIGHT_BYTES as u64 * LIGHT_BYTES as u64);
    if len == 0 {
        return Vec::new();
    }
    lights_from(&buffer.read(gpu, 0, len))
}

/// `GpuLight`s (LightBinnerFeature.Lights.cs, packed: 39 words) that light fog, as fog lights.
pub(crate) fn lights_from(bytes: &[u8]) -> Vec<FogLight> {
    bytes
        .as_chunks::<LIGHT_BYTES>()
        .0
        .iter()
        .filter_map(|l| {
            let w = words(l);
            // LightToWorld's rows hold forward/left/up/position as columns.
            let (forward, position) = (Vec3::new(w[3], w[7], w[11]), Vec3::new(w[6], w[10], w[14]));
            let (radius, fog, cone) = (w[25], w[33], (w[29], w[30]));
            let color = vec3(&w, 19) * fog;
            (radius > 0.0 && color.max_element() > 0.0).then(|| FogLight { position, radius, color, spot: (cone.0 != 0.0).then_some((forward, cone.0, cone.1)) })
        })
        .collect()
}

fn words(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|w| f32::from_le_bytes(*w)).collect()
}

fn vec3(w: &[f32], i: usize) -> Vec3 {
    let at = |k: usize| w.get(i + k).copied().unwrap_or(0.0);
    Vec3::new(at(0), at(1), at(2))
}

/// `DirectionalLightCB`'s sun (Shadows/DirectionalLightShadow.hlsl): toward it, colour × fog strength.
fn sun(bytes: &[u8]) -> Option<(Vec3, Vec3)> {
    let w = words(bytes);
    let enabled = w.get(79).is_some_and(|e| e.to_bits() != 0);
    enabled.then(|| (-vec3(&w, 4).normalize_or_zero(), vec3(&w, 0) * w[3]))
}

pub(crate) struct Constants(pub Vec<u8>);

impl Constants {
    fn f(&self, i: usize) -> f32 {
        self.0.get(i * 4..i * 4 + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
    }

    fn u(&self, i: usize) -> u32 {
        self.f(i).to_bits()
    }

    fn v3(&self, i: usize) -> Vec3 {
        Vec3::new(self.f(i), self.f(i + 1), self.f(i + 2))
    }

    /// A world-to-clip with the froxel rays' frustum: x and y over w span the corner rays, w is
    /// view depth (what managed's `g_mVolFogFromWorld` gives shading), z only keeps it invertible.
    pub(crate) fn view_proj(&self) -> Result<Mat4, String> {
        let (width, height) = (self.u(at::WIDTH_HEIGHT_DEPTH) as f32, self.u(at::WIDTH_HEIGHT_DEPTH + 1) as f32);
        let (dx, dy) = (self.v3(at::VIEW_DX) * width, self.v3(at::VIEW_DY) * height);
        let (pos, forward, near) = (self.v3(at::VIEW_POS), self.v3(at::VIEW_DIR), self.f(at::NEAR_FAR));
        if dx.length() < 1e-6 || dy.length() < 1e-6 || forward.length() < 1e-6 || near <= 0.0 {
            return Err("degenerate fog view".into());
        }
        let row = |axis: Vec3, scale: f32| (axis * scale).extend(-axis.dot(pos) * scale);
        let rows = [row(dx.normalize(), 2.0 / dx.length()), row(dy.normalize(), 2.0 / dy.length()), Vec4::new(0.0, 0.0, 0.0, near), row(forward.normalize(), 1.0)];
        Ok(Mat4::from_cols(rows[0], rows[1], rows[2], rows[3]).transpose())
    }

    pub(crate) fn volumes(&self) -> Vec<Volume> {
        (0..(self.u(at::VOLUME_COUNT) as usize).min(MAX_VOLUMES))
            .map(|i| {
                let m: [f32; 16] = std::array::from_fn(|k| self.f(at::BOX_WORLD_TO_VOLUME + 16 * i + k));
                Volume {
                    world_to_volume: Mat4::from_cols_array(&m).transpose(),
                    min: self.v3(at::BOX_MIN + 4 * i),
                    max: self.v3(at::BOX_MAX + 4 * i),
                    color: self.v3(at::BOX_COLOR_EXPONENT + 4 * i),
                    strength: self.f(at::BOX_MAX + 4 * i + 3),
                    exponent: self.f(at::BOX_COLOR_EXPONENT + 4 * i + 3),
                    spherical: self.f(at::BOX_MIN + 4 * i + 3) > 0.5,
                }
            })
            .collect()
    }

    pub(crate) fn settings(&self) -> Settings {
        let (scale, bias) = (self.f(at::FADE_IN_SCALE_BIAS), self.f(at::FADE_IN_SCALE_BIAS + 1));
        // Managed's fade is saturate(depth * scale + bias); scale 0 means none.
        let fade_in = if scale > 0.0 { (-bias / scale, (1.0 - bias) / scale) } else { (-2.0, -1.0) };
        Settings {
            draw_distance: self.f(at::NEAR_FAR + 2),
            fade_in,
            anisotropy: self.f(at::ANISOTROPY_SCATTERING),
            scattering: self.f(at::ANISOTROPY_SCATTERING + 1) / SCATTERING_SCALE,
        }
    }
}

#[cfg(test)]
#[path = "fog_managed_tests.rs"]
mod tests;
