//! A material program's resources for one draw, bound by name from reflection: the per-view
//! block, the instance transform buffer, other uniform blocks from material values and vfx
//! defaults, material textures, and stand-ins (1×1 white, zeroed buffers, default samplers) for
//! whatever the draw doesn't supply. Binding numbers are never assumed (research/shaders.md).

use super::defaults;
use super::layout::Value;
use super::program::{self, Layouts, Shading};
use super::shadow_slots;
use super::texel_words;
use super::view::{self, View};
use crate::render::lighting::{self, Lighting, shadow};
use crate::render::{attributes, bindless, buffers as gpu_buffers, fog, fog_volume, samplers, texture};
use crate::resource::Resource as _;
use glam::{Mat4, Vec4};
use render::wgpu::{self, BindingType as B};
use shaders::reflect::Binding;
use std::collections::HashMap;

/// `transform_buffer.fxc`'s `g_TransformBuffer`, 64 bytes per instance.
const TRANSFORMS: &str = "g_TransformBuffer";
/// `vs_decompress.fxc`'s skinned vertices (render/skin/cache.rs).
const VERTEX_CACHE: &str = "AnimatedVertexCache";

#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub world: Mat4,
    pub tint: [f32; 4],
}

/// Managed's analytic fog block, which the camera's attributes carry (`Lighting::fog`).
const FOG_BLOCK: &str = "FogConstants";
const FOG_VOLUME: &str = "g_tFogVolume";

/// What one draw brings to its material program.
pub struct Inputs<'a> {
    pub view: &'a View,
    pub lighting: &'a Lighting,
    pub instances: &'a [Instance],
    /// Material textures by HLSL name (`g_tColor`).
    pub textures: &'a HashMap<String, texture::Ref>,
    /// Material and draw values by HLSL or attribute name; draw values first.
    pub values: &'a [&'a HashMap<String, Vec4>],
    /// Render attributes (`SetRenderState`'s, then the context's), searched before `values` for
    /// values and textures by HLSL or attribute name, and for whole constant blocks by block name.
    pub attributes: &'a [attributes::Ref],
    /// `BindTexture` textures by slot, for texture bindings at that binding number nothing names.
    pub slots: &'a [(u32, texture::Ref)],
    /// The pass's own targets: never sampled (managed's shadow passes would read the map they write).
    pub targets: &'a [texture::Ref],
}

impl Inputs<'_> {
    fn constant_buffer(&self, block: &str) -> Option<std::sync::Arc<[u8]>> {
        self.attributes.iter().find_map(|a| attributes::constant_buffer(*a, block))
    }

    /// A GPU buffer an attribute names (managed's `g_TransformBuffer` for instanced draws).
    fn gpu_buffer(&self, name: &str) -> Option<wgpu::Buffer> {
        let handle = self.attributes.iter().find_map(|a| attributes::buffer(*a, name))?;
        gpu_buffers::get(gpu_buffers::Handle::new(handle)).map(|b| b.buffer.clone())
    }
}

/// The names a reflected resource answers to: its HLSL name and its vfx `Attribute( "..." )`.
pub(super) fn names<'a, P>(s: &'a Shading<P>, name: &'a str) -> impl Iterator<Item = &'a str> {
    std::iter::once(name).chain(s.attributes.get(name).and_then(|a| a.attribute.as_deref()))
}

fn transforms(instances: &[Instance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(instances.len().max(1) * 64);
    for i in instances {
        let rows = i.world.transpose().to_cols_array();
        out.extend(rows[..12].iter().flat_map(|f| f.to_le_bytes()));
        let [r, g, b, a] = i.tint.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u32);
        let _ = a;
        out.extend(i.tint[3].to_le_bytes());
        out.extend(((r << 16) | (g << 8) | b).to_le_bytes());
        out.extend([0u8; 8]);
    }
    out.resize(out.len().max(64), 0);
    out
}

pub(super) fn uniform_value<P>(s: &Shading<P>, inputs: &Inputs, name: &str) -> Option<Value> {
    let from_attributes = || inputs.attributes.iter().find_map(|a| names(s, name).find_map(|n| attributes::shader_value(*a, n)));
    let given = || inputs.values.iter().find_map(|m| names(s, name).find_map(|n| m.get(n).copied()));
    let attr = s.attributes.get(name);
    let source = || attr.and_then(|a| inputs.view.source(a.source.as_deref()?));
    let matrix = || inputs.attributes.iter().find_map(|a| names(s, name).find_map(|n| attributes::shader_matrix(*a, n))).map(Value::Mat);
    let vector = from_attributes().or_else(given).or_else(|| shadow_slots::indices(name)).or_else(|| fog::sun_value(inputs.lighting, name)).or_else(source);
    let unset_matrix = attr.is_some_and(|a| a.attribute.is_some() && a.default.is_none()).then_some(Value::Mat(Mat4::IDENTITY)); // real's world text sets no LayerMat
    vector.map(Value::Vec).or_else(matrix).or_else(|| attr.and_then(|a| a.default).map(Value::Vec)).or(unset_matrix)
}

/// Managed's bytes for a whole block, sized to the block.
fn block_bytes(bytes: &[u8], size: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.resize(size.max(16) as usize, 0);
    out
}

fn buffer(gpu: &render::Gpu, label: &str, usage: wgpu::BufferUsages, bytes: &[u8]) -> wgpu::Buffer {
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (bytes.len() as u64).next_multiple_of(16).max(16), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    gpu.queue.write_buffer(&b, 0, bytes);
    b
}

enum Resource {
    Buffer(wgpu::Buffer),
    View(wgpu::TextureView),
    Sampler(wgpu::Sampler),
    /// A bindless array, every element the same default.
    Views(Vec<wgpu::TextureView>),
    Samplers(Vec<wgpu::Sampler>),
}

/// The draw's texture for `b`: a render attribute's or the material's by name, managed's sun
/// cascade or the lighting's (shadow maps), else a `BindTexture` slot at its binding.
fn texture_view<P>(s: &Shading<P>, inputs: &Inputs, b: &Binding) -> Option<wgpu::TextureView> {
    let B::Texture { sample_type, view_dimension, multisampled: false } = b.ty else { return None };
    let from_attributes = || inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture(*a, n)));
    let from_material = || names(s, &b.name).find_map(|n| inputs.textures.get(n).copied());
    let from_slot = || inputs.slots.iter().find(|(slot, _)| *slot == b.binding).map(|(_, t)| *t);
    // A map also read without comparison has a `_cmp` clone for the compares (shaders' comparison pass).
    let base = b.name.strip_suffix("_cmp").unwrap_or(&b.name);
    let from_managed = || shadow::cascade_of(base).and_then(|n| shadow::managed_cascade(&inputs.constant_buffer("DirectionalLightCB")?, n));
    let t = from_attributes().or_else(from_material).or_else(from_managed).or_else(|| shadow_slots::texture(base)).or_else(|| inputs.lighting.texture(&b.name)).or_else(from_slot)?;
    // An error texture (every compiled texture, for now) would paint the draw magenta.
    let t = texture::resolve(t).filter(|t| !t.is_error())?;
    if inputs.targets.iter().filter_map(|r| texture::resolve(*r)).any(|r| std::sync::Arc::ptr_eq(&r, &t)) {
        return None;
    }
    let gpu = t.gpu.as_ref()?;
    let srgb = s.attributes.get(&b.name).is_some_and(|a| a.srgb);
    let slice = || inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture_slice(*a, n)));
    let view = || match slice().filter(|_| gpu.texture.mip_level_count() > 1) {
        Some(mip) => gpu.mip_sample_view(srgb, mip),
        None => gpu.sample_view(srgb).clone(),
    };
    (gpu.dimension() == view_dimension && sample_type_fits(gpu.desc.format, sample_type)).then(view)
}

/// Whether a texture of `format` can bind where a shader declares `want`.
pub fn sample_type_fits(format: wgpu::TextureFormat, want: wgpu::TextureSampleType) -> bool {
    match (format.sample_type(None, None), want) {
        (Some(wgpu::TextureSampleType::Float { filterable }), wgpu::TextureSampleType::Float { filterable: want }) => filterable || !want,
        (have, want) => have == Some(want),
    }
}

/// A bindless sampler array: element `i` is the sampler state managed registered at index `i`
/// (render/samplers.rs), else the default; comparison arrays keep the default.
fn bindless_samplers(gpu: &render::Gpu, b: &Binding, kind: wgpu::SamplerBindingType) -> Vec<wgpu::Sampler> {
    let fallback = defaults::sampler(gpu, kind, None, false);
    let real = |i| samplers::desc(i).filter(|_| kind != wgpu::SamplerBindingType::Comparison).map(|d| samplers::sampler(gpu, &d, kind));
    (0..program::array_len(b) as usize).map(|i| real(i).unwrap_or_else(|| fallback.clone())).collect()
}

/// A bindless texture array: element `i` is the texture managed's index `i` names
/// (render/bindless.rs) when it has the declared shape, else the default.
fn bindless_views(gpu: &render::Gpu, inputs: &Inputs, b: &Binding, dimension: wgpu::TextureViewDimension, sample_type: wgpu::TextureSampleType) -> Vec<wgpu::TextureView> {
    let declared = program::array_len(b);
    // Partially bound arrays may stop past the highest index handed out; others fill the declared length.
    let partial = gpu.device.features().contains(wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY);
    let len = if partial { declared.min(bindless::len()) } else { declared };
    let fallback = defaults::texture(gpu, dimension, sample_type);
    (0..len).map(|i| bindless::view(i, dimension, sample_type, inputs.targets).unwrap_or_else(|| fallback.clone())).collect()
}

/// Storage images whose bound texture has another format than the program declares, with that
/// format (downsample_spd's float4 mips over managed's Rg32Float pyramid).
pub fn storage_mismatches<P>(s: &Shading<P>, attrs: &[attributes::Ref]) -> Vec<program::StorageFormat> {
    let bound = |name: &str| {
        let (t, _) = attrs.iter().find_map(|a| names(s, name).find_map(|n| attributes::texture_mip(*a, n)))?;
        Some(texture::resolve(t)?.gpu.as_ref()?.desc.format)
    };
    s.bindings
        .iter()
        .filter_map(|b| match b.ty {
            B::StorageTexture { format, .. } => bound(&b.name).filter(|f| *f != format).map(|f| (b.group, b.binding, f)),
            _ => None,
        })
        .collect()
}

/// A `RWTexture2D`'s texture by name: the mip the attribute names (downsample_spd binds one per
/// mip) of a texture in exactly that format that allows storage writes (formats.rs gives that
/// usage where wgpu guarantees it).
fn storage_view<P>(s: &Shading<P>, inputs: &Inputs, b: &Binding, format: wgpu::TextureFormat) -> Result<wgpu::TextureView, String> {
    let found = inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture_mip(*a, n)));
    let (t, mip) = found.and_then(|(t, mip)| Some((texture::resolve(t)?, mip))).ok_or_else(|| format!("{}: no texture to write", b.name))?;
    let gpu = t.gpu.as_ref().ok_or_else(|| format!("{}: no texture to write", b.name))?;
    if gpu.desc.format != format || !gpu.texture.usage().contains(wgpu::TextureUsages::STORAGE_BINDING) {
        return Err(format!("{}: {:?} isn't writable as {format:?}", b.name, gpu.desc.format));
    }
    let mip = mip.min(gpu.texture.mip_level_count() - 1);
    Ok(gpu.texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: mip, mip_level_count: Some(1), ..Default::default() }))
}

fn resource<P>(gpu: &render::Gpu, s: &Shading<P>,inputs: &Inputs, b: &Binding) -> Result<Resource, String> {
    Ok(match b.ty {
        B::Buffer { ty: wgpu::BufferBindingType::Uniform, .. } => {
            let block = s.blocks.get(&(b.group, b.binding)).ok_or_else(|| format!("no layout for block {}", b.name))?;
            let view_fog = || (b.name == FOG_BLOCK).then(|| inputs.lighting.fog.clone()).flatten();
            let bytes = match (inputs.constant_buffer(&b.name).or_else(view_fog), b.name.as_str()) {
                (Some(given), _) => block_bytes(&given, block.size),
                (None, view::BLOCK) => block.fill(|n| inputs.view.constant(n)),
                (None, fog::BLOCK) => block.fill(|n| fog::constant(inputs.lighting, n).or_else(|| uniform_value(s, inputs, n))),
                (None, name) if lighting::BLOCKS.contains(&name) => block.fill(|n| inputs.lighting.constant(name, n)),
                (None, _) => block.fill(|n| uniform_value(s, inputs, n)),
            };
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::UNIFORM, &bytes))
        }
        B::Buffer { ty: wgpu::BufferBindingType::Storage { .. }, min_binding_size, .. } => {
            if let Some(given) = names(s, &b.name).find_map(|n| inputs.gpu_buffer(n)) {
                return Ok(Resource::Buffer(given));
            }
            if texel_words::image_of(&b.name).is_some() {
                return texel_words::bound(gpu, s, inputs, &b.name).map(|(_, words)| Resource::Buffer(words)).ok_or_else(|| format!("{}: no texture", b.name));
            }
            if names(s, &b.name).any(|n| n == VERTEX_CACHE)
                && let Some(cache) = crate::render::skin::cache::buffer()
            {
                return Ok(Resource::Buffer(cache.buffer.clone()));
            }
            let element = s.storage.get(&(b.group, b.binding));
            let mut bytes = match element.and_then(|e| inputs.lighting.storage(&b.name, e)) {
                Some(bytes) => bytes,
                None if b.name == TRANSFORMS => transforms(inputs.instances),
                None => vec![0; min_binding_size.map_or(256, |n| n.get().max(256)) as usize],
            };
            // An empty light list is shorter than one element (clustered_light_culling_cs binds 176).
            bytes.resize(bytes.len().max(min_binding_size.map_or(0, |n| n.get() as usize)), 0);
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::STORAGE, &bytes))
        }
        B::Texture { sample_type, view_dimension, .. } if b.count.is_some() => Resource::Views(bindless_views(gpu, inputs, b, view_dimension, sample_type)),
        B::Sampler(kind) if b.count.is_some() => Resource::Samplers(bindless_samplers(gpu, b, kind)),
        B::Texture { .. } if b.name == FOG_VOLUME => Resource::View(managed_fog(s, inputs, b).unwrap_or_else(|| fog_volume::view(gpu, inputs.lighting))),
        B::Texture { sample_type, view_dimension, .. } => {
            Resource::View(texture_view(s, inputs, b).unwrap_or_else(|| defaults::texture(gpu, view_dimension, sample_type)))
        }
        B::StorageTexture { format, view_dimension: wgpu::TextureViewDimension::D2, .. } => Resource::View(storage_view(s, inputs, b, format)?),
        B::Sampler(kind) => {
            let set = (kind != wgpu::SamplerBindingType::Comparison).then(|| inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::sampler(*a, n)))).flatten();
            let attr = s.attributes.get(&b.name);
            let fallback = || defaults::sampler(gpu, kind, attr.and_then(|a| a.compare), attr.is_some_and(|a| a.anisotropic));
            Resource::Sampler(set.map_or_else(fallback, |d| samplers::sampler(gpu, &d, kind)))
        }
        other => return Err(format!("{}: unsupported binding {other:?}", b.name)),
    })
}

/// The grid the `cs_volumetric_fog` stand-in marched for the fog texture managed's SceneRenderer
/// binds (render/fog_managed.rs).
fn managed_fog<P>(s: &Shading<P>, inputs: &Inputs, b: &Binding) -> Option<wgpu::TextureView> {
    let texture = inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture(*a, n)))?;
    crate::render::fog_managed::grid(texture.addr()).map(|f| f.view.clone())
}

/// One bind group per layout of the program, in group order.
pub fn bind_groups<P: Layouts>(gpu: &render::Gpu, s: &Shading<P>, inputs: &Inputs) -> Result<Vec<wgpu::BindGroup>, String> {
    let mut groups = Vec::new();
    for (g, layout) in s.program.bind_group_layouts().iter().enumerate() {
        let mine: Vec<&Binding> = s.bindings.iter().filter(|b| b.group == g as u32).collect();
        if super::bind_log::enabled(&s.label) {
            super::bind_log::describe(s, inputs, &mine);
        }
        let resources = mine.iter().map(|b| resource(gpu, s, inputs, b)).collect::<Result<Vec<_>, _>>()?;
        // wgpu's array resources borrow slices of references, which must outlive the entries.
        let arrays: Vec<(Vec<&wgpu::TextureView>, Vec<&wgpu::Sampler>)> = resources
            .iter()
            .map(|r| match r {
                Resource::Views(v) => (v.iter().collect(), Vec::new()),
                Resource::Samplers(v) => (Vec::new(), v.iter().collect()),
                _ => Default::default(),
            })
            .collect();
        let entries: Vec<_> = mine
            .iter()
            .zip(resources.iter().zip(&arrays))
            .map(|(b, (r, (views, samplers)))| wgpu::BindGroupEntry {
                binding: b.binding,
                resource: match r {
                    Resource::Buffer(buf) => buf.as_entire_binding(),
                    Resource::View(v) => wgpu::BindingResource::TextureView(v),
                    Resource::Sampler(smp) => wgpu::BindingResource::Sampler(smp),
                    Resource::Views(_) => wgpu::BindingResource::TextureViewArray(views),
                    Resource::Samplers(_) => wgpu::BindingResource::SamplerArray(samplers),
                },
            })
            .collect();
        groups.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(&s.label), layout, entries: &entries }));
    }
    Ok(groups)
}

#[cfg(test)]
#[path = "bind_tests.rs"]
mod tests;
