//! Stand-ins a material program binds when the draw doesn't supply a resource: a 1×1 texture per
//! (view dimension, sample type) — white for float textures, zero otherwise — and default
//! samplers (trilinear wrap; comparison samplers compare with the vfx `ComparisonFunc`, less-equal
//! by default).

use render::wgpu::{self, SamplerBindingType, TextureSampleType, TextureViewDimension as D};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static TEXTURES: LazyLock<Mutex<HashMap<(D, TextureSampleType), wgpu::TextureView>>> = LazyLock::new(Default::default);
type SamplerKey = (SamplerBindingType, Option<wgpu::CompareFunction>);
static SAMPLERS: LazyLock<Mutex<HashMap<SamplerKey, wgpu::Sampler>>> = LazyLock::new(Default::default);

fn format(sample: TextureSampleType) -> (wgpu::TextureFormat, u32) {
    match sample {
        TextureSampleType::Depth => (wgpu::TextureFormat::Depth32Float, 4),
        TextureSampleType::Uint => (wgpu::TextureFormat::Rgba32Uint, 16),
        TextureSampleType::Sint => (wgpu::TextureFormat::Rgba32Sint, 16),
        TextureSampleType::Float { .. } => (wgpu::TextureFormat::Rgba8Unorm, 4),
    }
}

fn create(gpu: &render::Gpu, dim: D, sample: TextureSampleType) -> wgpu::TextureView {
    let layers = match dim {
        D::Cube | D::CubeArray => 6,
        _ => 1,
    };
    let dimension = match dim {
        D::D1 => wgpu::TextureDimension::D1,
        D::D3 => wgpu::TextureDimension::D3,
        _ => wgpu::TextureDimension::D2,
    };
    let (format, texel) = format(sample);
    let depth = sample == TextureSampleType::Depth;
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | if depth { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::COPY_DST };
    let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor { label: Some("default"), size, mip_level_count: 1, sample_count: 1, dimension, format, usage, view_formats: &[] });
    if depth {
        clear_depth(gpu, &texture, layers);
    } else {
        let white = matches!(sample, TextureSampleType::Float { .. });
        let bytes = vec![if white { 255u8 } else { 0 }; (texel * layers) as usize];
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(texel), rows_per_image: Some(1) };
        gpu.queue.write_texture(texture.as_image_copy(), &bytes, layout, size);
    }
    texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dim), ..Default::default() })
}

/// Depth can't be written by copy: clear each layer to the far plane with a pass.
fn clear_depth(gpu: &render::Gpu, texture: &wgpu::Texture, layers: u32) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    for layer in 0..layers {
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(D::D2), base_array_layer: layer, array_layer_count: Some(1), ..Default::default() });
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store };
        let depth = wgpu::RenderPassDepthStencilAttachment { view: &view, depth_ops: Some(ops), stencil_ops: None };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("default depth"), depth_stencil_attachment: Some(depth), ..Default::default() });
    }
    gpu.queue.submit([encoder.finish()]);
}

pub fn texture(gpu: &render::Gpu, dim: D, sample: TextureSampleType) -> wgpu::TextureView {
    TEXTURES.lock().unwrap().entry((dim, sample)).or_insert_with(|| create(gpu, dim, sample)).clone()
}

pub fn sampler(gpu: &render::Gpu, kind: SamplerBindingType, compare: Option<wgpu::CompareFunction>) -> wgpu::Sampler {
    let mut cache = SAMPLERS.lock().unwrap();
    cache
        .entry((kind, compare))
        .or_insert_with(|| {
            let filter = if kind == SamplerBindingType::NonFiltering { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear };
            gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("default"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: if kind == SamplerBindingType::NonFiltering { wgpu::MipmapFilterMode::Nearest } else { wgpu::MipmapFilterMode::Linear },
                compare: (kind == SamplerBindingType::Comparison).then_some(compare.unwrap_or(wgpu::CompareFunction::LessEqual)),
                ..Default::default()
            })
        })
        .clone()
}
