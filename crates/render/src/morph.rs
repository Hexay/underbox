//! Morph composites drawn as real engine2 draws them: additive quads into one float target, one
//! draw per batch (real: one for position deltas, one for normal deltas). A quad covers a mesh's
//! region of the target and adds one morph target's weighted deltas, read from a buffer of
//! per-vertex `vec4`s (morph.wgsl).

use crate::state::{PipelineKey, RasterState, VertexBufferKey};
use crate::{Buffer, Gpu, Program, Texture};

pub struct Morpher {
    program: Program,
}

/// One target over one mesh region: `dst` is `[x, y, w, h]` texels of the composite, whose texel
/// `(x, y)` from the corner is vertex `y * w + x`; the target's deltas start at float4 `first`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub dst: [u32; 4],
    pub first: u32,
    pub vertices: u32,
    pub weight: f32,
}

/// One draw: `quads` reading `source`.
pub struct Batch<'a> {
    pub source: &'a Buffer,
    pub quads: Vec<Quad>,
}

const STRIDE: u64 = 32;

fn vertex_key() -> VertexBufferKey {
    let attr = |format, offset, shader_location| wgpu::VertexAttribute { format, offset, shader_location };
    let attributes = vec![attr(wgpu::VertexFormat::Float32x2, 0, 0), attr(wgpu::VertexFormat::Float32x2, 8, 1), attr(wgpu::VertexFormat::Float32x4, 16, 2)];
    VertexBufferKey { stride: STRIDE, step: wgpu::VertexStepMode::Vertex, attributes }
}

/// Four corners per quad: clip position, cell and params (morph.wgsl `In`).
fn corners(q: &Quad, size: [f32; 2]) -> Vec<f32> {
    let [x, y, w, h] = q.dst.map(|v| v as f32);
    let params = [q.first as f32, q.vertices as f32, w, q.weight];
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .into_iter()
        .flat_map(|(u, v)| {
            let clip = [(x + u * w) / size[0] * 2.0 - 1.0, 1.0 - (y + v * h) / size[1] * 2.0];
            [clip[0], clip[1], u * w, v * h].into_iter().chain(params)
        })
        .collect()
}

impl Morpher {
    pub fn new(gpu: &Gpu) -> Morpher {
        let storage = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None };
        let entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: storage, count: None };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("morph composite"), entries: &[entry] });
        Morpher { program: Program::new(gpu, "morph composite", include_str!("morph.wgsl"), vec![layout], ("vs_main", "fs_main")) }
    }

    /// Draw `batches` additively into `target` (a float colour target), cleared first when `clear`.
    pub fn composite(&self, gpu: &Gpu, target: &Texture, clear: bool, batches: &[Batch]) {
        let size = [target.desc.width as f32, target.desc.height as f32];
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let state = RasterState { depth_write: false, blend: Some(wgpu::BlendState { color: add, alpha: add }), ..Default::default() };
        let pipeline = self.program.pipeline_for(gpu, &PipelineKey { color: Some(target.desc.format), depth: None, vertex: vec![vertex_key()], state });
        let draws: Vec<_> = batches
            .iter()
            .filter(|b| !b.quads.is_empty())
            .map(|b| {
                let vertices: Vec<f32> = b.quads.iter().flat_map(|q| corners(q, size)).collect();
                let indices: Vec<u32> = (0..b.quads.len() as u32).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i)).collect();
                let group = gpu.bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("morph composite"),
                    layout: &self.program.bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: b.source.buffer.as_entire_binding() }],
                });
                let vb = gpu.upload(bytemuck::cast_slice(&vertices), 0);
                let ib = gpu.upload(bytemuck::cast_slice(&indices), 0);
                (group, vb, ib, indices.len() as u32)
            })
            .collect();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let load = if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("morph composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &target.target_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            for (group, vb, ib, n) in &draws {
                pass.set_bind_group(0, group, &[]);
                pass.set_vertex_buffer(0, vb.buffer.slice(vb.offset..vb.offset + vb.size));
                pass.set_index_buffer(ib.buffer.slice(ib.offset..ib.offset + ib.size), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*n, 0, 0..1);
            }
        }
        gpu.submit(encoder.finish());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_map_texels_to_clip_space() {
        let q = Quad { dst: [1, 1, 24, 1], first: 48, vertices: 24, weight: 0.5 };
        let v = corners(&q, [2048.0, 1024.0]);
        assert_eq!(v.len(), 32);
        assert!((v[0] - (2.0 / 2048.0 - 1.0)).abs() < 1e-6 && (v[1] - (1.0 - 2.0 / 1024.0)).abs() < 1e-6);
        assert_eq!(&v[2..8], &[0.0, 0.0, 48.0, 24.0, 24.0, 0.5]);
        assert_eq!(&v[2 + 16..4 + 16], &[24.0, 1.0]);
    }
}
