//! The fallback program for draws whose material program doesn't build or bind: position, vertex colour
//! and normal in, lit by one fixed light, times a per-draw tint. Attributes a mesh lacks are read
//! from a constant defaults buffer bound at stride 0 (white, no normal).

use crate::state::VertexBufferKey;
use crate::{Buffer, BufferKind, Gpu, Program};
use wgpu::VertexFormat as F;

/// Size of the per-draw uniform: `mvp`, `model` (two mat4x4) and `tint` (vec4).
pub const DRAW_UNIFORM_SIZE: u64 = 144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Semantic {
    Position,
    Color,
    Normal,
}

impl Semantic {
    fn location(self) -> u32 {
        self as u32
    }
}

/// Defaults buffer layout: colour (4 floats) at 0, normal (3 floats) at 16.
const DEFAULTS: [(Semantic, F, u64); 2] = [(Semantic::Color, F::Float32x4, 0), (Semantic::Normal, F::Float32x3, 16)];

pub struct Placeholder {
    pub program: Program,
    /// Bind at vertex slot 1 for every draw.
    pub defaults: Buffer,
}

impl Placeholder {
    pub fn new(gpu: &Gpu) -> Placeholder {
        let draw = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placeholder draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(DRAW_UNIFORM_SIZE) },
                count: None,
            }],
        });
        let program = Program::new(gpu, "placeholder", include_str!("placeholder.wgsl"), vec![draw], ("vs_main", "fs_main"));
        let values: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let defaults = Buffer::with_data(gpu, "placeholder defaults", BufferKind::Vertex, bytemuck::cast_slice(&values));
        Placeholder { program, defaults }
    }

    /// Slot 0 reads `attrs` (semantic, format, offset) from a mesh buffer of `stride`; slot 1 feeds
    /// whatever the mesh lacks from [`Placeholder::defaults`]. Position must be present.
    pub fn vertex_keys(stride: u64, attrs: &[(Semantic, F, u64)]) -> Vec<VertexBufferKey> {
        let attribute = |(s, format, offset): (Semantic, F, u64)| wgpu::VertexAttribute { format, offset, shader_location: s.location() };
        let mesh = attrs.iter().copied().map(attribute).collect();
        let missing = DEFAULTS.iter().copied().filter(|d| !attrs.iter().any(|a| a.0 == d.0)).map(attribute).collect();
        vec![
            VertexBufferKey { stride, step: wgpu::VertexStepMode::Vertex, attributes: mesh },
            VertexBufferKey { stride: 0, step: wgpu::VertexStepMode::Vertex, attributes: missing },
        ]
    }
}
