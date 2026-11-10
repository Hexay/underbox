//! Meshes in the placeholder vertex layout (position + colour).

use crate::{Buffer, BufferKind, Gpu};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

pub struct Mesh {
    pub vertices: Buffer,
    pub indices: Buffer,
    pub index_count: u32,
}

impl Mesh {
    pub fn new(gpu: &Gpu, vertices: &[Vertex], indices: &[u32]) -> Mesh {
        Mesh {
            vertices: Buffer::with_data(gpu, "mesh vertices", BufferKind::Vertex, bytemuck::cast_slice(vertices)),
            indices: Buffer::with_data(gpu, "mesh indices", BufferKind::Index, bytemuck::cast_slice(indices)),
            index_count: indices.len() as u32,
        }
    }

    /// An axis-aligned quad in the XY plane at `z`, two triangles, counter-clockwise.
    pub fn quad(gpu: &Gpu, min: [f32; 2], max: [f32; 2], z: f32, color: [f32; 4]) -> Mesh {
        let v = |x, y| Vertex { position: [x, y, z], color };
        let vertices = [v(min[0], min[1]), v(max[0], min[1]), v(max[0], max[1]), v(min[0], max[1])];
        Mesh::new(gpu, &vertices, &[0, 1, 2, 0, 2, 3])
    }
}
