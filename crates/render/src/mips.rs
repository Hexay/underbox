//! Mip chains drawn on the GPU (`IRenderContext::GenerateMipMaps`): each mip of each layer a 2x2
//! box of the one above (mips.wgsl), in the texture's own format, so an sRGB texture filters
//! decoded values and stores them encoded.

use crate::state::{PipelineKey, RasterState};
use crate::{Gpu, Program, Texture};

pub struct MipGenerator {
    program: Program,
}

impl MipGenerator {
    pub fn new(gpu: &Gpu) -> MipGenerator {
        let texture = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: texture, count: None };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("mips"), entries: &[entry] });
        MipGenerator { program: Program::new(gpu, "mips", include_str!("mips.wgsl"), vec![layout], ("vs_main", "fs_main")) }
    }

    /// Redraw mips 1 and up of every layer from mip 0. False when `t` can't be drawn into: a single
    /// mip, a volume, or a depth, integer or block-compressed format.
    pub fn generate(&self, gpu: &Gpu, t: &Texture) -> bool {
        let format = t.desc.format;
        let float = matches!(format.sample_type(None, None), Some(wgpu::TextureSampleType::Float { .. })) && !format.is_depth_stencil_format();
        if t.desc.mips <= 1 || t.desc.volume || !float || !t.texture.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            return false;
        }
        let key = PipelineKey { color: Some(format), depth: None, vertex: Vec::new(), state: RasterState { depth_write: false, ..Default::default() } };
        let pipeline = self.program.pipeline_for(gpu, &key);
        t.wrote();
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mips") });
        for mip in 1..t.desc.mips {
            for layer in 0..t.desc.layers.max(1) {
                let src = t.subresource_target_view(format, mip - 1, layer);
                let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mips"),
                    layout: &self.program.bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) }],
                });
                let dst = t.subresource_target_view(format, mip, layer);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mips"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &dst,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        gpu.submit(encoder.finish());
        true
    }
}
