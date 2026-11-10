//! A material program's resources for one draw, bound by name from reflection: the per-view
//! block, the instance transform buffer, other uniform blocks from material values and vfx
//! defaults, material textures, and stand-ins (1×1 white, zeroed buffers, default samplers) for
//! whatever the draw doesn't supply. Binding numbers are never assumed (research/shaders.md).

use super::defaults;
use super::layout::Value;
use super::program::Shading;
use super::view::{self, View};
use crate::render::texture;
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
    pub instances: &'a [Instance],
    /// Material textures by HLSL name (`g_tColor`).
    pub textures: &'a HashMap<String, texture::Ref>,
    /// Material and draw values by HLSL or attribute name; draw values first.
    pub values: &'a [&'a HashMap<String, Vec4>],
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

fn uniform_value(s: &Shading, inputs: &Inputs, name: &str) -> Option<Value> {
    let attr = s.attributes.get(name);
    let names = [Some(name), attr.and_then(|a| a.attribute.as_deref())];
    let given = inputs.values.iter().find_map(|m| names.iter().flatten().find_map(|n| m.get(*n)));
    given.copied().or_else(|| attr.and_then(|a| a.default)).map(Value::Vec)
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
}

fn material_view(inputs: &Inputs, b: &Binding) -> Option<wgpu::TextureView> {
    let B::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } = b.ty else { return None };
    // An error texture (every compiled texture, for now) would paint the draw magenta.
    let t = texture::resolve(*inputs.textures.get(&b.name)?).filter(|t| !t.is_error())?;
    let gpu = t.gpu.as_ref()?;
    let ok = match (gpu.desc.format.sample_type(None, None)?, sample_type) {
        (wgpu::TextureSampleType::Float { filterable }, wgpu::TextureSampleType::Float { filterable: want }) => filterable || !want,
        (have, want) => have == want,
    };
    ok.then(|| gpu.view.clone())
}

fn resource(gpu: &render::Gpu, s: &Shading, inputs: &Inputs, b: &Binding) -> Result<Resource, String> {
    Ok(match b.ty {
        B::Buffer { ty: wgpu::BufferBindingType::Uniform, .. } => {
            let block = s.blocks.get(&(b.group, b.binding)).ok_or_else(|| format!("no layout for block {}", b.name))?;
            let bytes = if b.name == view::BLOCK { block.fill(|n| inputs.view.constant(n)) } else { block.fill(|n| uniform_value(s, inputs, n)) };
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::UNIFORM, &bytes))
        }
        B::Buffer { ty: wgpu::BufferBindingType::Storage { .. }, min_binding_size, .. } => {
            let bytes = if b.name == TRANSFORMS { transforms(inputs.instances) } else { vec![0; min_binding_size.map_or(256, |n| n.get().max(256)) as usize] };
            Resource::Buffer(buffer(gpu, &b.name, wgpu::BufferUsages::STORAGE, &bytes))
        }
        B::Texture { sample_type, view_dimension, .. } => {
            Resource::View(material_view(inputs, b).unwrap_or_else(|| defaults::texture(gpu, view_dimension, sample_type)))
        }
        B::Sampler(kind) => Resource::Sampler(defaults::sampler(gpu, kind)),
        other => return Err(format!("{}: unsupported binding {other:?}", b.name)),
    })
}

/// One bind group per layout of the program, in group order.
pub fn bind_groups(gpu: &render::Gpu, s: &Shading, inputs: &Inputs) -> Result<Vec<wgpu::BindGroup>, String> {
    let mut groups = Vec::new();
    for (g, layout) in s.program.bind_group_layouts.iter().enumerate() {
        let mine: Vec<&Binding> = s.bindings.iter().filter(|b| b.group == g as u32).collect();
        let resources = mine.iter().map(|b| resource(gpu, s, inputs, b)).collect::<Result<Vec<_>, _>>()?;
        let entries: Vec<_> = mine
            .iter()
            .zip(&resources)
            .map(|(b, r)| wgpu::BindGroupEntry {
                binding: b.binding,
                resource: match r {
                    Resource::Buffer(buf) => buf.as_entire_binding(),
                    Resource::View(v) => wgpu::BindingResource::TextureView(v),
                    Resource::Sampler(smp) => wgpu::BindingResource::Sampler(smp),
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
