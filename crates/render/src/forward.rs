//! The basic forward pass: clear, then draw each item with the program's pipeline.

use crate::program::PLACEHOLDER_DRAW_SIZE;
use crate::{Gpu, Mesh, Program, Texture};
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
    pub program: Program,
}

impl ForwardPass {
    pub fn new(gpu: &Gpu) -> ForwardPass {
        ForwardPass { program: Program::placeholder(gpu) }
    }

    /// Draw `items` into `color` (and `depth`), clearing both first when `clear` is set.
    pub fn render(&self, gpu: &Gpu, color: &Texture, depth: &Texture, clear: Option<wgpu::Color>, camera: &Camera, items: &[DrawItem]) {
        let stride = PLACEHOLDER_DRAW_SIZE.next_multiple_of(u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment));
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("forward draws"),
            size: stride * items.len().max(1) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for (i, item) in items.iter().enumerate() {
            let mut data = [0f32; 20];
            data[..16].copy_from_slice(&(camera.view_proj * item.transform).to_cols_array());
            data[16..].copy_from_slice(&item.tint);
            gpu.queue.write_buffer(&uniforms, stride * i as u64, bytemuck::cast_slice(&data));
        }
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("forward draws"),
            layout: &self.program.bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(PLACEHOLDER_DRAW_SIZE),
                }),
            }],
        });
        let pipeline = self.program.pipeline(gpu, color.desc.format);
        let load = clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear);
        let depth_load = if clear.is_some() { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("forward"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.view,
                    depth_ops: Some(wgpu::Operations { load: depth_load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            for (i, item) in items.iter().enumerate() {
                pass.set_bind_group(0, &bind_group, &[(stride * i as u64) as u32]);
                pass.set_vertex_buffer(0, item.mesh.vertices.buffer.slice(..));
                pass.set_index_buffer(item.mesh.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}
