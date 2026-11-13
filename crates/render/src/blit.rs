//! Scaled and format-converting blits: what a texture copy can't do (different formats or sizes),
//! drawn as a full-screen triangle into the destination rect (blit.wgsl).

use crate::state::{PipelineKey, RasterState};
use crate::{Gpu, Program, Texture};

pub struct Blitter {
    /// By [`Transfer`].
    programs: [Program; 3],
    nearest: wgpu::Sampler,
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
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(16) };
        let sampler = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering);
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("blit"), entries: &[entry(0, texture), entry(1, uniform), entry(2, sampler)] });
        let program = |fs| Program::new(gpu, "blit", include_str!("blit.wgsl"), vec![layout.clone()], ("vs_main", fs));
        let clamp = wgpu::AddressMode::ClampToEdge;
        let nearest = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("blit"), address_mode_u: clamp, address_mode_v: clamp, ..Default::default() });
        Blitter { programs: [program("fs_copy"), program("fs_encode"), program("fs_decode")], nearest }
    }

    /// Draw `src_rect` of `src` into `dst_rect` of `dst` (both `[x, y, w, h]` in texels). False
    /// when either side can't take part (depth, integer formats, an empty or outside rect).
    pub fn blit(&self, gpu: &Gpu, src: &Texture, src_rect: [u32; 4], dst: &Texture, dst_rect: [u32; 4], transfer: Transfer) -> bool {
        let float = |t: &Texture| matches!(t.desc.format.sample_type(None, None), Some(wgpu::TextureSampleType::Float { .. })) && !t.desc.format.is_depth_stencil_format();
        let [x, y, w, h] = dst_rect;
        if !float(src) || !float(dst) || src.dimension() != wgpu::TextureViewDimension::D2 || w == 0 || h == 0 || x + w > dst.desc.width || y + h > dst.desc.height || src_rect[2] == 0 || src_rect[3] == 0 {
            return false;
        }
        // Encode through the target's sRGB view, as native's blit writes an sRGB target; lavapipe's
        // format conversion costs less than the shader's pow.
        let srgb_target = (transfer == Transfer::Encode).then(|| dst.srgb_target_view()).flatten();
        let (target, format, transfer) = match &srgb_target {
            Some(view) => (view, dst.desc.format.add_srgb_suffix(), Transfer::None),
            None => (&dst.target_view, dst.desc.format, transfer),
        };
        let program = &self.programs[transfer as usize];
        let params = src_rect.map(|v| v as f32);
        let uniform = gpu.upload(bytemuck::cast_slice(&params), 0);
        let bind_group = gpu.bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &program.bind_group_layouts[0],
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 1, resource: uniform.binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
            ],
        });
        let state = RasterState { depth_write: false, ..Default::default() };
        let key = PipelineKey { color: Some(format), depth: None, vertex: Vec::new(), state };
        let pipeline = program.pipeline_for(gpu, &key);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
        gpu.submit(encoder.finish());
        true
    }
}
