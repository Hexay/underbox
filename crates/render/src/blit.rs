//! Scaled and format-converting blits: what a texture copy can't do (different formats or sizes),
//! drawn as a full-screen triangle into the destination rect (blit.wgsl).

use crate::state::{PipelineKey, RasterState};
use crate::{Gpu, Program, Texture};

pub struct Blitter {
    program: Program,
}

/// The sRGB transfer a blit applies (`RenderTools.BlitTexture`'s `nSrgbTransfer`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transfer {
    #[default]
    None,
    /// Linear → sRGB-encoded values.
    Encode,
    /// sRGB-encoded → linear values.
    Decode,
}

impl Blitter {
    pub fn new(gpu: &Gpu) -> Blitter {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let texture = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(32) };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("blit"), entries: &[entry(0, texture), entry(1, uniform)] });
        Blitter { program: Program::new(gpu, "blit", include_str!("blit.wgsl"), vec![layout], ("vs_main", "fs_main")) }
    }

    /// Draw `src_rect` of `src` into `dst_rect` of `dst` (both `[x, y, w, h]` in texels). False
    /// when either side can't take part (depth, integer formats, an empty or outside rect).
    pub fn blit(&self, gpu: &Gpu, src: &Texture, src_rect: [u32; 4], dst: &Texture, dst_rect: [u32; 4], transfer: Transfer) -> bool {
        let float = |t: &Texture| matches!(t.desc.format.sample_type(None, None), Some(wgpu::TextureSampleType::Float { .. })) && !t.desc.format.is_depth_stencil_format();
        let [x, y, w, h] = dst_rect;
        if !float(src) || !float(dst) || w == 0 || h == 0 || x + w > dst.desc.width || y + h > dst.desc.height || src_rect[2] == 0 || src_rect[3] == 0 {
            return false;
        }
        let [sx, sy, sw, sh] = src_rect.map(|v| v as f32);
        let params = [sx, sy, sw, sh, transfer as u32 as f32, 0.0, 0.0, 0.0];
        let uniform = crate::Buffer::with_data(gpu, "blit params", crate::BufferKind::Uniform, bytemuck::cast_slice(&params));
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.program.bind_group_layouts[0],
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 1, resource: uniform.buffer.as_entire_binding() },
            ],
        });
        let state = RasterState { depth_write: false, ..Default::default() };
        let key = PipelineKey { color: Some(dst.desc.format), depth: None, vertex: Vec::new(), state };
        let pipeline = self.program.pipeline_for(gpu, &key);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        true
    }
}
