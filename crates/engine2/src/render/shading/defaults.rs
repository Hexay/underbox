//! Stand-ins a material program binds when the draw doesn't supply a resource: a 1×1 texture per
//! (view dimension, sample type) — white for float textures, zero otherwise, and transparent
//! black for cubes as real's `SCENE_TXTR_BLACK_CUBE_1x1` (sRGB) — and default
//! samplers (the vfx declaration's filter and addressing, trilinear wrap without one; comparison
//! samplers compare with the vfx `ComparisonFunc`, greater-equal by default, as reverse-Z maps want).

use super::sampler_state::SamplerState;
use render::wgpu::{self, SamplerBindingType, TextureSampleType, TextureViewDimension as D};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static TEXTURES: LazyLock<Mutex<HashMap<(D, TextureSampleType), wgpu::TextureView>>> = LazyLock::new(Default::default);
type SamplerKey = (SamplerBindingType, Option<wgpu::CompareFunction>, u16, SamplerState);
static SAMPLERS: LazyLock<Mutex<HashMap<SamplerKey, wgpu::Sampler>>> = LazyLock::new(Default::default);

fn format(sample: TextureSampleType, cube: bool) -> (wgpu::TextureFormat, u32) {
    match sample {
        TextureSampleType::Depth => (wgpu::TextureFormat::Depth32Float, 4),
        TextureSampleType::Uint => (wgpu::TextureFormat::Rgba32Uint, 16),
        TextureSampleType::Sint => (wgpu::TextureFormat::Rgba32Sint, 16),
        TextureSampleType::Float { .. } if cube => (wgpu::TextureFormat::Rgba8UnormSrgb, 4),
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
    let cube = layers == 6;
    let (format, texel) = format(sample, cube);
    let depth = sample == TextureSampleType::Depth;
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | if depth { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::COPY_DST };
    let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor { label: Some("default"), size, mip_level_count: 1, sample_count: 1, dimension, format, usage, view_formats: &[] });
    if depth {
        clear_depth(gpu, &texture, layers);
    } else {
        let white = matches!(sample, TextureSampleType::Float { .. }) && !cube;
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
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(crate::render::depth::SHADOW.clear()), store: wgpu::StoreOp::Store };
        let depth = wgpu::RenderPassDepthStencilAttachment { view: &view, depth_ops: Some(ops), stencil_ops: None };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("default depth"), depth_stencil_attachment: Some(depth), ..Default::default() });
    }
    gpu.submit(encoder.finish());
}

pub fn texture(gpu: &render::Gpu, dim: D, sample: TextureSampleType) -> wgpu::TextureView {
    TEXTURES.lock().unwrap().entry((dim, sample)).or_insert_with(|| create(gpu, dim, sample)).clone()
}

/// The sampler a binding of `kind` gets from its vfx declaration: `state`, anisotropic up to
/// `r_max_anisotropy`; a non-filtering binding samples nearest whatever it declares.
pub fn sampler(gpu: &render::Gpu, kind: SamplerBindingType, compare: Option<wgpu::CompareFunction>, mut state: SamplerState) -> wgpu::Sampler {
    let anisotropy = if state.anisotropic && kind == SamplerBindingType::Filtering { crate::render::convars::max_anisotropy() } else { 1 };
    if kind == SamplerBindingType::NonFiltering {
        state.filter = [wgpu::FilterMode::Nearest; 2];
        state.mip = wgpu::MipmapFilterMode::Nearest;
    }
    // wgpu requires all-linear filtering for anisotropy.
    if anisotropy > 1 {
        state.filter = [wgpu::FilterMode::Linear; 2];
        state.mip = wgpu::MipmapFilterMode::Linear;
    }
    let mut cache = SAMPLERS.lock().unwrap();
    cache
        .entry((kind, compare, anisotropy, state))
        .or_insert_with(|| {
            let border_ok = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
            gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                compare: (kind == SamplerBindingType::Comparison).then_some(compare.unwrap_or(crate::render::depth::SHADOW.compare())),
                anisotropy_clamp: anisotropy,
                ..state.descriptor(border_ok)
            })
        })
        .clone()
}
