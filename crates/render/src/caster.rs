//! The depth-only shadow caster program: a mesh's positions on slot 0, a world matrix per instance
//! on slot 1, one view-projection for the pass. Every caster sharing a mesh draws as one instanced
//! draw, as native's shadow views do.

use crate::state::VertexBufferKey;
use crate::{Gpu, Program, Stage};
use glam::Mat4;

/// Bytes of one instance: a column-major world matrix.
pub const INSTANCE_STRIDE: u64 = 64;

pub struct Caster {
    pub program: Program,
}

impl Caster {
    pub fn new(gpu: &Gpu) -> Caster {
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow caster"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(64) },
                count: None,
            }],
        });
        let vertex = Stage::new(gpu, "shadow caster", include_str!("caster.wgsl"), "vs_main");
        Caster { program: Program::from_stages(gpu, "shadow caster", vertex, None, vec![layout]) }
    }

    /// Slot 0 keeps only `mesh`'s position (location 0); slot 1 steps per instance.
    pub fn vertex_keys(mesh: &VertexBufferKey) -> Vec<VertexBufferKey> {
        let position = mesh.attributes.iter().filter(|a| a.shader_location == 0).copied().collect();
        let world = (0..4).map(|i| wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16 * i, shader_location: 1 + i as u32 }).collect();
        vec![
            VertexBufferKey { stride: mesh.stride, step: wgpu::VertexStepMode::Vertex, attributes: position },
            VertexBufferKey { stride: INSTANCE_STRIDE, step: wgpu::VertexStepMode::Instance, attributes: world },
        ]
    }

    pub fn bind_group(&self, gpu: &Gpu, view_proj: Mat4) -> wgpu::BindGroup {
        let buffer = crate::Buffer::with_data(gpu, "shadow caster view", crate::BufferKind::Uniform, bytemuck::cast_slice(&view_proj.to_cols_array()));
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow caster"),
            layout: &self.program.bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.buffer.as_entire_binding() }],
        })
    }

    /// The instance stream for `worlds`.
    pub fn instances(gpu: &Gpu, worlds: &[Mat4]) -> crate::Buffer {
        let floats: Vec<f32> = worlds.iter().flat_map(|m| m.to_cols_array()).collect();
        crate::Buffer::with_data(gpu, "shadow caster instances", crate::BufferKind::Vertex, bytemuck::cast_slice(&floats))
    }
}
