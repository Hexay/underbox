//! A shader program: one WGSL module, its entry points and bind-group layouts, and the render
//! pipelines built from it per target format. Translated vfx combos plug in here as further
//! `Program`s whose layouts come from reflection; see docs/renderer.md, "Shaders".

use crate::{DEPTH_FORMAT, Gpu, Vertex};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Program {
    pub label: String,
    pub module: wgpu::ShaderModule,
    pub bind_group_layouts: Vec<wgpu::BindGroupLayout>,
    layout: wgpu::PipelineLayout,
    vertex_entry: &'static str,
    fragment_entry: &'static str,
    pipelines: Mutex<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
}

/// Size of the placeholder's per-draw uniform: `mvp: mat4x4` + `tint: vec4`.
pub(crate) const PLACEHOLDER_DRAW_SIZE: u64 = 80;

impl Program {
    pub fn new(
        gpu: &Gpu,
        label: &str,
        wgsl: &str,
        bind_group_layouts: Vec<wgpu::BindGroupLayout>,
        entries: (&'static str, &'static str),
    ) -> Program {
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let refs: Vec<_> = bind_group_layouts.iter().map(Some).collect();
        let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &refs,
            immediate_size: 0,
        });
        Program {
            label: label.into(),
            module,
            bind_group_layouts,
            layout,
            vertex_entry: entries.0,
            fragment_entry: entries.1,
            pipelines: Mutex::default(),
        }
    }

    /// Vertex colour times a per-draw tint, transformed by a per-draw MVP (dynamic offset).
    pub fn placeholder(gpu: &Gpu) -> Program {
        let draw = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placeholder draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(PLACEHOLDER_DRAW_SIZE),
                },
                count: None,
            }],
        });
        Program::new(gpu, "placeholder", include_str!("placeholder.wgsl"), vec![draw], ("vs_main", "fs_main"))
    }

    /// The pipeline drawing into `color` + [`DEPTH_FORMAT`], built on first use.
    pub fn pipeline(&self, gpu: &Gpu, color: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut cache = self.pipelines.lock().unwrap();
        cache.entry(color).or_insert_with(|| self.build(gpu, color)).clone()
    }

    fn build(&self, gpu: &Gpu, color: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&self.label),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some(self.vertex_entry),
                compilation_options: Default::default(),
                buffers: &[Some(Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: Some(self.fragment_entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: color, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}
