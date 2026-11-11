//! The froxel march on the GPU (fog_volume.wgsl): lights each froxel, blends it with the previous
//! frame's froxel at the same world position (reprojected), and integrates front to back.

use super::fog_volume::{FogLight, Frustum, SIZE, Settings, Sources, Volume, bytes};
use crate::render::device;
use crate::render::shading::view::View;
use glam::{Mat4, Vec3};
use render::wgpu;
use render::wgpu::util::DeviceExt as _;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

const WORKGROUP: [u32; 2] = [8, 8];
const MAX_LIGHTS: usize = 16;
/// `Anisotropy` 1 (managed's default) is this asymmetry; a true 1 is a singularity.
const MAX_ASYMMETRY: f32 = 0.6;
/// How much of a froxel's lighting is this frame's; the rest is its reprojected history.
const CURRENT_WEIGHT: f32 = 0.3;
/// Histories kept at most (one per view target or managed fog texture); more clears them all.
const MAX_HISTORIES: usize = 32;

/// The last march under a key: its world-to-clip, slicing and per-froxel lighting.
struct History {
    world_to_clip: Mat4,
    near: f32,
    range: f32,
    lighting: wgpu::TextureView,
}

static HISTORIES: LazyLock<Mutex<HashMap<usize, History>>> = LazyLock::new(Default::default);

fn pipeline(gpu: &render::Gpu) -> &'static wgpu::ComputePipeline {
    static PIPELINE: OnceLock<wgpu::ComputePipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| {
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fog_volume"), source: wgpu::ShaderSource::Wgsl(include_str!("fog_volume.wgsl").into()) });
        gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("fog_volume"), layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None })
    })
}

fn params_bytes(view_proj: Mat4, sources: &Sources, settings: &Settings, (near, range): (f32, f32), counts: [u32; 2], history: Option<&History>, shadow: Option<Mat4>) -> Vec<u8> {
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
    out.extend([counts[0], counts[1], u32::from(history.is_some()), u32::from(shadow.is_some())].iter().flat_map(|c| c.to_le_bytes()));
    out.extend(bytes(history.map_or(Mat4::IDENTITY, |h| h.world_to_clip).to_cols_array()));
    out.extend(bytes(history.map_or([1.0, 0.0, 1.0, 0.0], |h| [CURRENT_WEIGHT, h.near, h.range, 0.0])));
    out.extend(bytes(shadow.unwrap_or(Mat4::IDENTITY).to_cols_array()));
    out
}

/// The sun's world → shadow-map transform and map, when it has one on the GPU.
fn shadow_map(sources: &Sources) -> Option<(Mat4, Arc<crate::render::texture::Texture>)> {
    let s = sources.shadow?;
    Some((s.world_to_shadow, crate::render::texture::resolve(s.map).filter(|t| t.gpu.is_some())?))
}

/// Bound in place of a shadow map; the march never reads it (`counts.w` is 0).
fn no_shadow(gpu: &render::Gpu) -> &'static wgpu::TextureView {
    static NONE: OnceLock<wgpu::TextureView> = OnceLock::new();
    NONE.get_or_init(|| {
        let desc = wgpu::TextureDescriptor {
            label: Some("no fog shadow"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        gpu.device.create_texture(&desc).create_view(&Default::default())
    })
}

fn light_bytes(sources: &Sources) -> (Vec<u8>, u32) {
    let lit = &sources.lights[..sources.lights.len().min(MAX_LIGHTS)];
    let mut out: Vec<u8> = bytes(lit.iter().flat_map(FogLight::floats));
    // A storage binding can't be empty.
    out.resize(out.len().max(48), 0);
    (out, lit.len() as u32)
}

fn froxel_texture(gpu: &render::Gpu, label: &str, size: [u32; 3]) -> wgpu::TextureView {
    let [width, height, depth] = size;
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: depth },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// March `volumes` for a view; `None` when nothing could show (no volumes, no device, or an
/// orthographic view, whose w carries no depth for the froxel mapping). `history` keys the
/// previous frame's lighting to blend with (0: none).
pub fn march(volumes: &[Volume], view_proj: Mat4, sources: &Sources, settings: &Settings, history: usize) -> Option<Arc<Frustum>> {
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
    let mut histories = HISTORIES.lock().unwrap();
    let previous = (history != 0).then(|| histories.get(&history)).flatten();
    let (lights, light_count) = light_bytes(sources);
    let shadow = shadow_map(sources);
    let uniform = params_bytes(view_proj, sources, settings, (near, range), [volumes.len() as u32, light_count], previous, shadow.as_ref().map(|s| s.0));
    let shadow_view = shadow.as_ref().and_then(|s| s.1.gpu.as_ref()).map_or_else(|| no_shadow(gpu), |t| &t.view);
    let volume_data = bytes(volumes.iter().flat_map(Volume::floats));
    let buffer = |label, usage, contents: &[u8]| gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
    let (uniform, volume_data, lights) = (buffer("fog params", wgpu::BufferUsages::UNIFORM, &uniform), buffer("fog volumes", wgpu::BufferUsages::STORAGE, &volume_data), buffer("fog lights", wgpu::BufferUsages::STORAGE, &lights));
    let view = froxel_texture(gpu, "fog froxels", SIZE);
    let lighting = froxel_texture(gpu, "fog froxel lighting", SIZE);
    static NO_HISTORY: OnceLock<wgpu::TextureView> = OnceLock::new();
    let previous_lighting = previous.map_or_else(|| NO_HISTORY.get_or_init(|| froxel_texture(gpu, "no fog history", [1, 1, 1])), |h| &h.lighting);
    let pipeline = pipeline(gpu);
    let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fog_volume"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            entry(0, uniform.as_entire_binding()),
            entry(1, volume_data.as_entire_binding()),
            entry(2, lights.as_entire_binding()),
            entry(3, wgpu::BindingResource::TextureView(&view)),
            entry(4, wgpu::BindingResource::TextureView(previous_lighting)),
            entry(5, wgpu::BindingResource::TextureView(&lighting)),
            entry(6, wgpu::BindingResource::TextureView(shadow_view)),
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(SIZE[0].div_ceil(WORKGROUP[0]), SIZE[1].div_ceil(WORKGROUP[1]), 1);
    }
    gpu.submit(encoder.finish());
    if history != 0 {
        if histories.len() >= MAX_HISTORIES && !histories.contains_key(&history) {
            histories.clear();
        }
        histories.insert(history, History { world_to_clip: view_proj, near, range, lighting });
    }
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
