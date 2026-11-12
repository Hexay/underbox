//! One render pass of placeholder-shaded draws into a colour and/or depth target. Every draw
//! carries its own vertex/index buffers, raster state and per-draw uniforms (dynamic offsets).

use crate::placeholder::{DRAW_UNIFORM_SIZE, Placeholder};
use crate::state::{PipelineKey, RasterState, VertexBufferKey};
use crate::{Gpu, Program, Texture};
use glam::Mat4;
use std::ops::Range;

/// What a pass draws into, and the part of it.
#[derive(Clone, Copy, Default)]
pub struct Targets<'a> {
    pub color: Option<&'a Texture>,
    pub depth: Option<&'a Texture>,
    /// The depth target's array layer (a cube face) drawn into.
    pub depth_layer: u32,
    /// `x, y, w, h, min_z, max_z` in pixels; the whole target when `None`.
    pub viewport: Option<[f32; 6]>,
    pub scissor: Option<[u32; 4]>,
    /// Write colour through its sRGB twin (encoded), where the format has one.
    pub srgb_write: bool,
}

impl Targets<'_> {
    pub fn size(&self) -> Option<(u32, u32)> {
        self.color.or(self.depth).map(|t| (t.desc.width, t.desc.height))
    }

    /// The colour attachment's format as pipelines see it: the sRGB twin under [`Targets::srgb_write`].
    pub fn color_format(&self) -> Option<wgpu::TextureFormat> {
        let t = self.color?;
        Some(crate::texture::srgb_twin(t.desc.format).filter(|_| self.srgb_write && t.desc.render_target).unwrap_or(t.desc.format))
    }
}

/// Clear values; `None` keeps what the target holds.
#[derive(Clone, Copy, Default)]
pub struct Load {
    pub color: Option<wgpu::Color>,
    pub depth: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct DrawUniforms {
    pub mvp: Mat4,
    pub model: Mat4,
    pub tint: [f32; 4],
}

impl DrawUniforms {
    fn bytes(&self) -> [f32; 36] {
        let mut d = [0f32; 36];
        d[..16].copy_from_slice(&self.mvp.to_cols_array());
        d[16..32].copy_from_slice(&self.model.to_cols_array());
        d[32..].copy_from_slice(&self.tint);
        d
    }
}

/// A draw shaded by its own program (a translated material) instead of the placeholder.
pub struct Shaded<'a> {
    pub program: &'a Program,
    /// Bound at group index = position; the caller built them against `program`'s layouts.
    pub bind_groups: &'a [wgpu::BindGroup],
    /// Vertex slots 1.. (instance streams, attribute defaults), laid out as `vertex[1..]`.
    pub buffers: Vec<(&'a wgpu::Buffer, u64)>,
}

pub struct MeshDraw<'a> {
    /// Slot 0: the mesh's vertex buffer and byte offset; its layout is `vertex[0]`.
    pub vertices: (&'a wgpu::Buffer, u64),
    /// From [`Placeholder::vertex_keys`], or the material program's inputs.
    pub vertex: Vec<VertexBufferKey>,
    pub indices: Option<(&'a wgpu::Buffer, wgpu::IndexFormat)>,
    /// Index range when indexed, else vertex range.
    pub range: Range<u32>,
    pub base_vertex: i32,
    pub instances: u32,
    /// The placeholder's per-draw data; unused by [`Shaded`] draws.
    pub uniforms: DrawUniforms,
    pub state: RasterState,
    pub shaded: Option<Shaded<'a>>,
}

/// Run one pass: load or clear the targets, then draw `draws` in order. A pass that would neither
/// clear nor draw isn't recorded.
pub fn execute(gpu: &Gpu, placeholder: &Placeholder, targets: &Targets, load: Load, draws: &[MeshDraw]) {
    let Some((width, height)) = targets.size() else { return };
    if draws.is_empty() && load.color.is_none() && load.depth.is_none() {
        return;
    }
    let stride = DRAW_UNIFORM_SIZE.next_multiple_of(u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment));
    let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("draw uniforms"),
        size: stride * draws.len().max(1) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for (i, d) in draws.iter().enumerate() {
        gpu.queue.write_buffer(&uniforms, stride * i as u64, bytemuck::cast_slice(&d.uniforms.bytes()));
    }
    let program = &placeholder.program;
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("draw uniforms"),
        layout: &program.bind_group_layouts[0],
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uniforms, offset: 0, size: wgpu::BufferSize::new(DRAW_UNIFORM_SIZE) }),
        }],
    });
    let store = wgpu::StoreOp::Store;
    let srgb_view = targets.color.filter(|_| targets.srgb_write).and_then(Texture::srgb_target_view);
    let color = targets.color.map(|t| wgpu::RenderPassColorAttachment {
        view: srgb_view.as_ref().unwrap_or(&t.target_view),
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: load.color.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store },
    });
    let layer_view = targets.depth.filter(|_| targets.depth_layer > 0).map(|t| t.layer_target_view(targets.depth_layer));
    let depth = targets.depth.map(|t| wgpu::RenderPassDepthStencilAttachment {
        view: layer_view.as_ref().unwrap_or(&t.target_view),
        depth_ops: Some(wgpu::Operations { load: load.depth.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store }),
        stencil_ops: t.desc.format.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store }),
    });
    let color_count = usize::from(color.is_some());
    let colors = [color];
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("draws"),
            color_attachments: &colors[..color_count],
            depth_stencil_attachment: depth,
            ..Default::default()
        });
        if let Some([x, y, w, h, near, far]) = targets.viewport {
            pass.set_viewport(x, y, w.min(width as f32 - x), h.min(height as f32 - y), near, far);
        }
        if let Some([x, y, w, h]) = targets.scissor {
            let (x, y) = (x.min(width), y.min(height));
            pass.set_scissor_rect(x, y, w.min(width - x), h.min(height - y));
        }
        for (i, d) in draws.iter().enumerate() {
            let key = PipelineKey { color: targets.color_format(), depth: targets.depth.map(|t| t.desc.format), vertex: d.vertex.clone(), state: d.state };
            pass.set_vertex_buffer(0, d.vertices.0.slice(d.vertices.1..));
            if let Some(s) = d.state.stencil {
                pass.set_stencil_reference(s.reference);
            }
            match &d.shaded {
                Some(s) => {
                    pass.set_pipeline(&s.program.pipeline_for(gpu, &key));
                    for (g, bg) in s.bind_groups.iter().enumerate() {
                        pass.set_bind_group(g as u32, bg, &[]);
                    }
                    for (slot, (b, offset)) in s.buffers.iter().enumerate() {
                        pass.set_vertex_buffer(slot as u32 + 1, b.slice(*offset..));
                    }
                }
                None => {
                    pass.set_pipeline(&program.pipeline_for(gpu, &key));
                    pass.set_bind_group(0, &bind_group, &[(stride * i as u64) as u32]);
                    pass.set_vertex_buffer(1, placeholder.defaults.buffer.slice(..));
                }
            }
            match d.indices {
                Some((buffer, format)) => {
                    pass.set_index_buffer(buffer.slice(..), format);
                    pass.draw_indexed(d.range.clone(), d.base_vertex, 0..d.instances);
                }
                None => pass.draw(d.range.clone(), 0..d.instances),
            }
        }
    }
    gpu.submit(encoder.finish());
}
