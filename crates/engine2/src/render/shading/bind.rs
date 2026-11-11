//! A material program's resources for one draw, bound by name from reflection: the per-view
//! block, the instance transform buffer, other uniform blocks from material values and vfx
//! defaults, material textures, and stand-ins (1×1 white, zeroed buffers, default samplers) for
//! whatever the draw doesn't supply. Binding numbers are never assumed (research/shaders.md).

use super::defaults;
use super::layout::Value;
use super::program::{self, Layouts, Shading};
use super::view::{self, View};
use crate::render::lighting::{self, Lighting, shadow};
use crate::render::{attributes, buffers as gpu_buffers, texture};
use crate::resource::Resource as _;
use glam::{Mat4, Vec4};
use render::wgpu::{self, BindingType as B};
use shaders::reflect::Binding;
use std::collections::HashMap;

/// `transform_buffer.fxc`'s `g_TransformBuffer`, 64 bytes per instance.
const TRANSFORMS: &str = "g_TransformBuffer";

#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub world: Mat4,
    pub tint: [f32; 4],
}

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
fn names<'a, P>(s: &'a Shading<P>, name: &'a str) -> impl Iterator<Item = &'a str> {
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

fn uniform_value<P>(s: &Shading<P>,inputs: &Inputs, name: &str) -> Option<Value> {
    let from_attributes = || inputs.attributes.iter().find_map(|a| names(s, name).find_map(|n| attributes::shader_value(*a, n)));
    let given = || inputs.values.iter().find_map(|m| names(s, name).find_map(|n| m.get(n).copied()));
    let default = || s.attributes.get(name).and_then(|a| a.default);
    from_attributes().or_else(given).or_else(default).map(Value::Vec)
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
    let B::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } = b.ty else { return None };
    let from_attributes = || inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture(*a, n)));
    let from_material = || names(s, &b.name).find_map(|n| inputs.textures.get(n).copied());
    let from_slot = || inputs.slots.iter().find(|(slot, _)| *slot == b.binding).map(|(_, t)| *t);
    // A map also read without comparison has a `_cmp` clone for the compares (shaders' comparison pass).
    let cascade = shadow::cascade_of(b.name.strip_suffix("_cmp").unwrap_or(&b.name));
    let from_managed = || cascade.and_then(|n| shadow::managed_cascade(&inputs.constant_buffer("DirectionalLightCB")?, n));
    let t = from_attributes().or_else(from_material).or_else(from_managed).or_else(|| inputs.lighting.texture(&b.name)).or_else(from_slot)?;
    // An error texture (every compiled texture, for now) would paint the draw magenta.
    let t = texture::resolve(t).filter(|t| !t.is_error())?;
    if inputs.targets.iter().filter_map(|r| texture::resolve(*r)).any(|r| std::sync::Arc::ptr_eq(&r, &t)) {
        return None;
    }
    let gpu = t.gpu.as_ref()?;
    let ok = match (gpu.desc.format.sample_type(None, None)?, sample_type) {
        (wgpu::TextureSampleType::Float { filterable }, wgpu::TextureSampleType::Float { filterable: want }) => filterable || !want,
        (have, want) => have == want,
    };
    ok.then(|| gpu.view.clone())
}

/// A `RWTexture2D`'s texture by name: mip 0 of a texture in exactly that format that allows
/// storage writes (formats.rs gives that usage where wgpu guarantees it).
fn storage_view<P>(s: &Shading<P>, inputs: &Inputs, b: &Binding, format: wgpu::TextureFormat) -> Result<wgpu::TextureView, String> {
    let t = inputs.attributes.iter().find_map(|a| names(s, &b.name).find_map(|n| attributes::texture(*a, n))).and_then(texture::resolve);
    let gpu = t.as_ref().and_then(|t| t.gpu.as_ref()).ok_or_else(|| format!("{}: no texture to write", b.name))?;
    if gpu.desc.format != format || !gpu.texture.usage().contains(wgpu::TextureUsages::STORAGE_BINDING) {
        return Err(format!("{}: {:?} isn't writable as {format:?}", b.name, gpu.desc.format));
    }
    Ok(gpu.texture.create_view(&wgpu::TextureViewDescriptor { mip_level_count: Some(1), ..Default::default() }))
}

fn resource<P>(gpu: &render::Gpu, s: &Shading<P>,inputs: &Inputs, b: &Binding) -> Result<Resource, String> {
    Ok(match b.ty {
        B::Buffer { ty: wgpu::BufferBindingType::Uniform, .. } => {
            let block = s.blocks.get(&(b.group, b.binding)).ok_or_else(|| format!("no layout for block {}", b.name))?;
            let bytes = match (inputs.constant_buffer(&b.name), b.name.as_str()) {
                (Some(given), _) => block_bytes(&given, block.size),
                (None, view::BLOCK) => block.fill(|n| inputs.view.constant(n)),
                (None, name) if lighting::BLOCKS.contains(&name) => block.fill(|n| inputs.lighting.constant(name, n)),
                (None, _) => block.fill(|n| uniform_value(s, inputs, n)),
            };
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::UNIFORM, &bytes))
        }
        B::Buffer { ty: wgpu::BufferBindingType::Storage { .. }, min_binding_size, .. } => {
            if let Some(given) = inputs.gpu_buffer(&b.name) {
                return Ok(Resource::Buffer(given));
            }
            let element = s.storage.get(&(b.group, b.binding));
            let bytes = match element.and_then(|e| inputs.lighting.storage(&b.name, e)) {
                Some(bytes) => bytes,
                None if b.name == TRANSFORMS => transforms(inputs.instances),
                None => vec![0; min_binding_size.map_or(256, |n| n.get().max(256)) as usize],
            };
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::STORAGE, &bytes))
        }
        // TODO: managed's bindless indices name its global descriptor set; elements are defaults.
        B::Texture { sample_type, view_dimension, .. } if b.count.is_some() => {
            Resource::Views(vec![defaults::texture(gpu, view_dimension, sample_type); program::array_len(b) as usize])
        }
        B::Sampler(kind) if b.count.is_some() => Resource::Samplers(vec![defaults::sampler(gpu, kind, None); program::array_len(b) as usize]),
        B::Texture { sample_type, view_dimension, .. } => {
            Resource::View(texture_view(s, inputs, b).unwrap_or_else(|| defaults::texture(gpu, view_dimension, sample_type)))
        }
        B::StorageTexture { format, view_dimension: wgpu::TextureViewDimension::D2, .. } => Resource::View(storage_view(s, inputs, b, format)?),
        B::Sampler(kind) => Resource::Sampler(defaults::sampler(gpu, kind, s.attributes.get(&b.name).and_then(|a| a.compare))),
        other => return Err(format!("{}: unsupported binding {other:?}", b.name)),
    })
}

/// One bind group per layout of the program, in group order.
pub fn bind_groups<P: Layouts>(gpu: &render::Gpu, s: &Shading<P>, inputs: &Inputs) -> Result<Vec<wgpu::BindGroup>, String> {
    let mut groups = Vec::new();
    for (g, layout) in s.program.bind_group_layouts().iter().enumerate() {
        let mine: Vec<&Binding> = s.bindings.iter().filter(|b| b.group == g as u32).collect();
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
mod tests {
    use super::*;

    #[test]
    fn transform_rows_and_packed_tint() {
        let world = Mat4::from_translation(glam::Vec3::new(1.0, 2.0, 3.0));
        let bytes = transforms(&[Instance { world, tint: [1.0, 0.5, 0.0, 0.25] }]);
        let f = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        // Row0 = (1, 0, 0, tx), Row1 = (0, 1, 0, ty), Row2 = (0, 0, 1, tz).
        assert_eq!([f(0), f(3), f(5), f(7), f(10), f(11)], [1.0, 1.0, 1.0, 2.0, 1.0, 3.0]);
        assert_eq!(f(12), 0.25);
        assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), 0xFF8000);
    }
}
