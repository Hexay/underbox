//! The basic forward pass over [`Mesh`]es: clear, then draw each item with the placeholder.

use crate::pass::{self, DrawUniforms, Load, MeshDraw, Targets};
use crate::placeholder::{Placeholder, Semantic};
use crate::state::RasterState;
use crate::{Gpu, Mesh, Texture, Vertex};
use glam::Mat4;
use glam::camera::rh::{proj::directx as proj, view};

/// Right-handed, Y-up view space; wgpu's clip depth is 0..1, as in Direct3D.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub view_proj: Mat4,
}

impl Camera {
    /// Orthographic over `[-half_w, half_w] × [-half_h, half_h]`, looking down -Z from z = 0.
    pub fn ortho(half_w: f32, half_h: f32, near: f32, far: f32) -> Camera {
        Camera { view_proj: proj::orthographic(-half_w, half_w, -half_h, half_h, near, far) }
    }

    pub fn perspective(fov_y_radians: f32, aspect: f32, near: f32, eye: glam::Vec3, target: glam::Vec3) -> Camera {
        let proj = proj::perspective_infinite(fov_y_radians, aspect, near);
        Camera { view_proj: proj * view::look_at_mat4(eye, target, glam::Vec3::Y) }
    }
}

pub struct DrawItem<'a> {
    pub mesh: &'a Mesh,
    pub transform: Mat4,
    pub tint: [f32; 4],
}

pub struct ForwardPass {
    pub placeholder: Placeholder,
}

impl ForwardPass {
    pub fn new(gpu: &Gpu) -> ForwardPass {
        ForwardPass { placeholder: Placeholder::new(gpu) }
    }

    /// Draw `items` into `color` (and `depth`), clearing both first when `clear` is set.
    pub fn render(&self, gpu: &Gpu, color: &Texture, depth: &Texture, clear: Option<wgpu::Color>, camera: &Camera, items: &[DrawItem]) {
        let layout = Placeholder::vertex_keys(size_of::<Vertex>() as u64, &[(Semantic::Position, wgpu::VertexFormat::Float32x3, 0), (Semantic::Color, wgpu::VertexFormat::Float32x4, 12)]);
        let draws: Vec<_> = items
            .iter()
            .map(|item| MeshDraw {
                vertices: (&item.mesh.vertices.buffer, 0),
                vertex: layout.clone(),
                indices: Some((&item.mesh.indices.buffer, wgpu::IndexFormat::Uint32)),
                range: 0..item.mesh.index_count,
                base_vertex: 0,
                instances: 1,
                uniforms: DrawUniforms { mvp: camera.view_proj * item.transform, model: item.transform, tint: item.tint },
                state: RasterState::default(),
                shaded: None,
            })
            .collect();
        let targets = Targets { color: Some(color), depth: Some(depth), ..Default::default() };
        let load = Load { color: clear, depth: clear.map(|_| 1.0) };
        pass::execute(gpu, &self.placeholder, &targets, load, &draws);
    }
}
