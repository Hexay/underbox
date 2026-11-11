//! Volumetric fog: the volumes managed adds per world (`AddVolumetricFogVolume`) and the froxel
//! grid a view renders from them (fog_march.rs). Native only ships the result, so the grid,
//! density scale and phase are ours; `volumetric_fog.fxc` samples it. See docs/fog.md.

use crate::render::lighting::Lighting;
use crate::render::lighting::shadow::SunShadow;
use crate::scene::world;
use abi::structs::{SceneVolumetricFogParameters, SceneVolumetricFogVolume};
use glam::{Mat4, Vec3};
use render::wgpu;
use render::wgpu::util::DeviceExt as _;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

#[cfg(test)]
pub use super::fog_march::froxel;
pub use super::fog_march::march;

pub(crate) const SIZE: [u32; 3] = [96, 56, 64];
/// World units of fog at strength 1 that take transmittance to 1/e.
const UNITS_PER_OPTICAL_DEPTH: f32 = 256.0;

static VOLUMES: LazyLock<Mutex<HashMap<usize, Vec<SceneVolumetricFogVolume>>>> = LazyLock::new(Default::default);

/// Keep `volume` under `id` until [`remove`].
pub fn add(world: usize, id: u32, mut volume: SceneVolumetricFogVolume) {
    volume.m_u_id = id;
    VOLUMES.lock().unwrap().entry(world).or_default().push(volume);
}

pub fn remove(world: usize, id: u32) {
    if let Some(list) = VOLUMES.lock().unwrap().get_mut(&world) {
        list.retain(|v| v.m_u_id != id);
    }
}

/// A rendered grid and the mapping `g_mVolFogFromWorld` hands shaders.
#[derive(Debug)]
pub struct Frustum {
    pub world_to_clip: Mat4,
    pub near: f32,
    pub range: f32,
    pub view: wgpu::TextureView,
}

impl PartialEq for Frustum {
    fn eq(&self, other: &Frustum) -> bool {
        std::ptr::eq(self, other)
    }
}

/// What `volumetric_fog.fxc` samples: the view's grid, else no fog (black, opaque). Managed's own
/// `FogVolume` is a volume texture without a GPU side here, and its writer `cs_volumetric_fog`
/// ships without source, so its binding would sample the white default and wash the frame out.
pub fn view(gpu: &render::Gpu, lighting: &Lighting) -> wgpu::TextureView {
    static CLEAR: OnceLock<wgpu::TextureView> = OnceLock::new();
    let clear = || {
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("no fog"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            &[0, 0, 0, 255],
        );
        texture.create_view(&Default::default())
    };
    lighting.fog_volume.as_ref().map_or_else(|| CLEAR.get_or_init(clear).clone(), |f| f.view.clone())
}

pub(crate) fn bytes(floats: impl IntoIterator<Item = f32>) -> Vec<u8> {
    floats.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// A fog volume: local bounds, placed by `world_to_volume`.
pub struct Volume {
    pub world_to_volume: Mat4,
    pub min: Vec3,
    pub max: Vec3,
    pub color: Vec3,
    pub strength: f32,
    pub exponent: f32,
    pub spherical: bool,
}

impl Volume {
    pub(crate) fn native(v: &SceneVolumetricFogVolume) -> Volume {
        let v3 = |v: abi::structs::Vector3| Vec3::new(v.vec.x, v.vec.y, v.vec.z);
        let world_to_volume = crate::render::scene::camera::mat4(v.m_mat_world_to_volume);
        Volume { world_to_volume, min: v3(v.m_v_min), max: v3(v.m_v_max), color: v3(v.m_v_color), strength: v.m_f_strength, exponent: v.m_f_exponent, spherical: v.m_b_spherical != 0 }
    }

    pub(crate) fn floats(&self) -> Vec<f32> {
        let (center, half) = ((self.min + self.max) * 0.5, ((self.max - self.min) * 0.5).max(Vec3::splat(1e-3)));
        let mut out = self.world_to_volume.to_cols_array().to_vec();
        out.extend(center.extend(self.strength / UNITS_PER_OPTICAL_DEPTH).to_array());
        out.extend(half.extend(self.exponent).to_array());
        out.extend(self.color.extend(f32::from(self.spherical)).to_array());
        out
    }
}

/// What lights the medium: ambient (already × `IndirectStrength`), the sun (direction toward it,
/// colour × its fog strength), the sun's shadow map when it casts shadows, and local lights.
#[derive(Default)]
pub struct Sources {
    pub ambient: Vec3,
    pub sun: Option<(Vec3, Vec3)>,
    pub shadow: Option<SunShadow>,
    pub lights: Vec<FogLight>,
}

/// A point or spot light in the medium; `color` is already × its fog strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogLight {
    pub position: Vec3,
    pub radius: f32,
    pub color: Vec3,
    /// A spot's direction and its inner and outer cone cosines.
    pub spot: Option<(Vec3, f32, f32)>,
}

impl FogLight {
    pub(crate) fn floats(&self) -> [f32; 12] {
        // A point light's cone test passes everywhere: smoothstep(-2, -1, cosine) is 1.
        let (direction, inner, outer) = self.spot.unwrap_or((Vec3::X, -1.0, -2.0));
        let mut out = [0.0; 12];
        out[..4].copy_from_slice(&self.position.extend(self.radius).to_array());
        out[4..8].copy_from_slice(&self.color.extend(outer).to_array());
        out[8..].copy_from_slice(&direction.normalize_or_zero().extend(inner.max(outer + 1e-4)).to_array());
        out
    }
}

impl Sources {
    fn of(lighting: &Lighting, indirect_strength: f32) -> Sources {
        let cosines = |(inner, outer): (f32, f32)| (inner.to_radians().cos(), outer.to_radians().cos());
        let lights = lighting
            .lights
            .iter()
            .filter(|l| l.radius > 0.0 && l.fog > 0.0)
            .map(|l| FogLight { position: l.position, radius: l.radius, color: l.color * l.fog, spot: l.cone.map(|c| { let (i, o) = cosines(c); (l.forward(), i, o) }) })
            .collect();
        let sun = lighting.sun.as_ref().map(|s| (-s.forward(), s.color * s.fog));
        Sources { ambient: lighting.ambient.truncate() * indirect_strength, sun, shadow: lighting.shadow.filter(|_| sun.is_some()), lights }
    }
}

/// `SceneVolumetricFogParameters`' march settings.
pub struct Settings {
    pub draw_distance: f32,
    pub fade_in: (f32, f32),
    pub anisotropy: f32,
    pub scattering: f32,
}

/// March `worlds`' volumes for one view; `None` when nothing could show (see [`march`]).
/// `history` keys the view's previous frame (its colour target), 0 for none.
pub fn render(worlds: &[world::Ref], view_proj: Mat4, lighting: &Lighting, params: &SceneVolumetricFogParameters, history: usize) -> Option<Arc<Frustum>> {
    let volumes: Vec<Volume> = {
        let all = VOLUMES.lock().unwrap();
        worlds.iter().filter_map(|w| all.get(&w.addr())).flatten().map(Volume::native).collect()
    };
    let settings = Settings {
        draw_distance: params.m_fl_draw_distance,
        fade_in: (params.m_fl_fade_in_start, params.m_fl_fade_in_end),
        anisotropy: params.m_fl_anisotropy,
        scattering: params.m_fl_scattering,
    };
    march(&volumes, view_proj, &Sources::of(lighting, params.m_fl_indirect_strength), &settings, history)
}
