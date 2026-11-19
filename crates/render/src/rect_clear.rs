//! Clears of part of a target (native's `Clear` under a viewport or scissor smaller than it): a
//! scissored draw that writes the colour, or the depth and stencil 0, whatever was there.

use crate::state::{PipelineKey, RasterState, Stencil};
use crate::{Gpu, Program, Texture};

pub struct RectClearer {
    program: Program,
}

impl RectClearer {
    pub fn new(gpu: &Gpu) -> RectClearer {
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(32) };
        let entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: uniform, count: None };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("rect clear"), entries: &[entry] });
        RectClearer { program: Program::new(gpu, "rect clear", include_str!("rect_clear.wgsl"), vec![layout], ("vs_main", "fs_main")) }
    }

    /// Clear `[x, y, w, h]` of mip 0, array layer `layer`, as [`Texture::clear_layer`] clears all of
    /// it. False (nothing drawn) for volumes, integer colour formats and an empty or outside rect.
    pub fn clear(&self, gpu: &Gpu, t: &Texture, layer: u32, [x, y, w, h]: [u32; 4], color: wgpu::Color, depth: f32) -> bool {
        let format = t.desc.format;
        let is_depth = format.is_depth_stencil_format();
        let float = matches!(format.sample_type(None, None), Some(wgpu::TextureSampleType::Float { .. }));
        if t.desc.volume || (!is_depth && !float) || w == 0 || h == 0 || x + w > t.desc.width || y + h > t.desc.height {
            return false;
        }
        let params = [color.r, color.g, color.b, color.a, f64::from(depth), 0.0, 0.0, 0.0].map(|v| v as f32);
        let uniform = gpu.upload(bytemuck::cast_slice(&params), 0);
        let bind_group = gpu.bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect clear"),
            layout: &self.program.bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.binding() }],
        });
        let replace = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, pass_op: wgpu::StencilOperation::Replace, ..wgpu::StencilFaceState::IGNORE };
        let state = match is_depth {
            true => RasterState { depth_compare: wgpu::CompareFunction::Always, stencil: Some(Stencil { front: replace, back: replace, ..Default::default() }), ..Default::default() },
            false => RasterState { depth_write: false, ..Default::default() },
        };
        let key = PipelineKey { color: (!is_depth).then_some(format), depth: is_depth.then_some(format), vertex: Vec::new(), state };
        let pipeline = self.program.pipeline_for(gpu, &key);
        let view = t.layer_target_view(layer);
        let store = wgpu::StoreOp::Store;
        let color_attachment = [Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store } })];
        let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
            view: &view,
            depth_ops: format.has_depth_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store }),
            stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store }),
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rect clear"),
                color_attachments: if is_depth { &[] } else { &color_attachment },
                depth_stencil_attachment: is_depth.then_some(depth_attachment),
                ..Default::default()
            });
            pass.set_scissor_rect(x, y, w, h);
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_stencil_reference(0);
            pass.draw(0..3, 0..1);
        }
        gpu.submit(encoder.finish());
        true
    }
}
