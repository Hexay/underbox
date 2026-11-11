//! Volumetric fog: the volumes managed adds per world (`AddVolumetricFogVolume`) and the froxel
//! grid a view renders from them. Native only ships the result, so the grid, density scale and
//! phase are ours; `volumetric_fog.fxc` samples it. See docs/fog.md.

use crate::render::device;
use crate::render::lighting::Lighting;
use crate::render::shading::view::View;
use crate::scene::world;
use abi::structs::{SceneVolumetricFogParameters, SceneVolumetricFogVolume};
use glam::{Mat4, Vec3};
use render::wgpu;
use render::wgpu::util::DeviceExt as _;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

const SIZE: [u32; 3] = [96, 56, 64];
const WORKGROUP: [u32; 2] = [8, 8];
const MAX_LIGHTS: usize = 16;
/// World units of fog at strength 1 that take transmittance to 1/e.
const UNITS_PER_OPTICAL_DEPTH: f32 = 256.0;
/// `Anisotropy` 1 (managed's default) is this asymmetry; a true 1 is a singularity.
const MAX_ASYMMETRY: f32 = 0.6;

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

fn pipeline(gpu: &render::Gpu) -> &'static wgpu::ComputePipeline {
    static PIPELINE: OnceLock<wgpu::ComputePipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| {
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fog_volume"), source: wgpu::ShaderSource::Wgsl(include_str!("fog_volume.wgsl").into()) });
        gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("fog_volume"), layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None })
    })
}

fn bytes(floats: impl IntoIterator<Item = f32>) -> Vec<u8> {
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

    fn floats(&self) -> Vec<f32> {
        let (center, half) = ((self.min + self.max) * 0.5, ((self.max - self.min) * 0.5).max(Vec3::splat(1e-3)));
        let mut out = self.world_to_volume.to_cols_array().to_vec();
        out.extend(center.extend(self.strength / UNITS_PER_OPTICAL_DEPTH).to_array());
        out.extend(half.extend(self.exponent).to_array());
        out.extend(self.color.extend(f32::from(self.spherical)).to_array());
        out
    }
}

/// What lights the medium: ambient (already × `IndirectStrength`), the sun (direction toward it,
/// colour × its fog strength) and local lights.
#[derive(Default)]
pub struct Sources {
    pub ambient: Vec3,
    pub sun: Option<(Vec3, Vec3)>,
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
    fn floats(&self) -> [f32; 12] {
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
        Sources { ambient: lighting.ambient.truncate() * indirect_strength, sun: lighting.sun.as_ref().map(|s| (-s.forward(), s.color * s.fog)), lights }
    }
}

/// `SceneVolumetricFogParameters`' march settings.
pub struct Settings {
    pub draw_distance: f32,
    pub fade_in: (f32, f32),
    pub anisotropy: f32,
    pub scattering: f32,
}

fn params_bytes(view_proj: Mat4, sources: &Sources, settings: &Settings, (near, range): (f32, f32), counts: [u32; 2]) -> Vec<u8> {
    let view = View { view_proj, viewport: [0.0; 4], target: [1.0; 2], tone_map: 1.0 };
    let asymmetry = settings.anisotropy.clamp(-1.0, 1.0) * MAX_ASYMMETRY;
    let (sun_direction, sun_color) = sources.sun.unwrap_or((Vec3::Z, Vec3::ZERO));
    let (fade_start, fade_end) = settings.fade_in;
    let mut out = bytes(view_proj.inverse().to_cols_array());
    out.extend(bytes(view.camera_position().truncate().extend(near).to_array()));
    out.extend(bytes(view.camera_axes().0.truncate().extend(range).to_array()));
    out.extend(bytes([fade_start, fade_end.max(fade_start + 1e-3), asymmetry, settings.scattering]));
    out.extend(bytes(sources.ambient.extend(0.0).to_array()));
    out.extend(bytes(sun_direction.extend(0.0).to_array()));
    out.extend(bytes(sun_color.extend(0.0).to_array()));
    out.extend([counts[0], counts[1], 0, 0].iter().flat_map(|c| c.to_le_bytes()));
    out
}

fn light_bytes(sources: &Sources) -> (Vec<u8>, u32) {
    let lit = &sources.lights[..sources.lights.len().min(MAX_LIGHTS)];
    let mut out: Vec<u8> = bytes(lit.iter().flat_map(FogLight::floats));
    // A storage binding can't be empty.
    out.resize(out.len().max(48), 0);
    (out, lit.len() as u32)
}

/// March `worlds`' volumes for one view; `None` when nothing could show (see [`march`]).
pub fn render(worlds: &[world::Ref], view_proj: Mat4, lighting: &Lighting, params: &SceneVolumetricFogParameters) -> Option<Arc<Frustum>> {
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
    march(&volumes, view_proj, &Sources::of(lighting, params.m_fl_indirect_strength), &settings)
}

/// March `volumes` for a view; `None` when nothing could show (no volumes, no device, or an
/// orthographic view, whose w carries no depth for the froxel mapping).
pub fn march(volumes: &[Volume], view_proj: Mat4, sources: &Sources, settings: &Settings) -> Option<Arc<Frustum>> {
    let gpu = device::gpu()?;
    let perspective = view_proj.row(3).truncate() != Vec3::ZERO;
    let far = settings.draw_distance;
    if volumes.is_empty() || !perspective || far <= 0.0 {
        return None;
    }
    // Native's slicing (managed's SceneRenderer port, Pipeline/Fog/VolumetricFog.cs): the first slice
    // starts one slice in, so either side's constants address this grid.
    let near = far / SIZE[2] as f32;
    let range = far - near;
    let (lights, light_count) = light_bytes(sources);
    let uniform = params_bytes(view_proj, sources, settings, (near, range), [volumes.len() as u32, light_count]);
    let volume_data = bytes(volumes.iter().flat_map(Volume::floats));
    let buffer = |label, usage, contents: &[u8]| gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
    let (uniform, volume_data, lights) = (buffer("fog params", wgpu::BufferUsages::UNIFORM, &uniform), buffer("fog volumes", wgpu::BufferUsages::STORAGE, &volume_data), buffer("fog lights", wgpu::BufferUsages::STORAGE, &lights));
    let [width, height, depth] = SIZE;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fog froxels"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: depth },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let pipeline = pipeline(gpu);
    let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fog_volume"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[entry(0, uniform.as_entire_binding()), entry(1, volume_data.as_entire_binding()), entry(2, lights.as_entire_binding()), entry(3, wgpu::BindingResource::TextureView(&view))],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(width.div_ceil(WORKGROUP[0]), height.div_ceil(WORKGROUP[1]), 1);
    }
    gpu.submit(encoder.finish());
    Some(Arc::new(Frustum { world_to_clip: view_proj, near, range, view }))
}

/// Froxel `(x, y, z)`'s in-scattering and transmittance, read back.
#[cfg(test)]
pub fn froxel(f: &Frustum, [x, y, z]: [u32; 3]) -> [f32; 4] {
    let gpu = device::gpu().unwrap();
    let staging = render::Buffer::new(gpu, "froxel", render::BufferKind::Vertex, 256);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let origin = wgpu::Origin3d { x, y, z };
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: f.view.texture(), mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &staging.buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None } },
        wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
    );
    gpu.submit(encoder.finish());
    let bytes = staging.read(gpu, 0, 8);
    std::array::from_fn(|i| half::f16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]).to_f32())
}
