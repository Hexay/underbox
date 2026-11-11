//! A shader program: vertex and fragment stages (one WGSL module or one per stage, as the
//! shader translator emits them), its bind-group layouts, and the render pipelines built from it
//! per [`PipelineKey`]. See docs/renderer.md, "Shaders".

use crate::state::{PipelineKey, RasterState, VertexBufferKey};
use crate::{DEPTH_FORMAT, Gpu, Vertex};
use std::collections::HashMap;
use std::sync::Mutex;

/// One compiled stage: its module and entry point.
pub struct Stage {
    pub module: wgpu::ShaderModule,
    pub entry: String,
}

impl Stage {
    pub fn new(gpu: &Gpu, label: &str, wgsl: &str, entry: &str) -> Stage {
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        Stage { module, entry: entry.into() }
    }
}

pub struct Program {
    pub label: String,
    pub vertex: Stage,
    /// `None` for depth-only programs.
    pub fragment: Option<Stage>,
    pub bind_group_layouts: Vec<wgpu::BindGroupLayout>,
    layout: wgpu::PipelineLayout,
    pipelines: Mutex<HashMap<PipelineKey, wgpu::RenderPipeline>>,
    failed: Mutex<HashMap<PipelineKey, String>>,
}

impl Program {
    /// Both stages from one WGSL module.
    pub fn new(gpu: &Gpu, label: &str, wgsl: &str, bind_group_layouts: Vec<wgpu::BindGroupLayout>, entries: (&str, &str)) -> Program {
        let vertex = Stage::new(gpu, label, wgsl, entries.0);
        let fragment = Stage { module: vertex.module.clone(), entry: entries.1.into() };
        Program::from_stages(gpu, label, vertex, Some(fragment), bind_group_layouts)
    }

    /// Separately compiled stages, as translated vfx combos come (one module per stage).
    pub fn from_stages(gpu: &Gpu, label: &str, vertex: Stage, fragment: Option<Stage>, bind_group_layouts: Vec<wgpu::BindGroupLayout>) -> Program {
        let refs: Vec<_> = bind_group_layouts.iter().map(Some).collect();
        let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &refs, immediate_size: 0 });
        Program { label: label.into(), vertex, fragment, bind_group_layouts, layout, pipelines: Mutex::default(), failed: Mutex::default() }
    }

    /// The pipeline drawing [`Vertex`] meshes into `color` + [`DEPTH_FORMAT`] with default state.
    pub fn pipeline(&self, gpu: &Gpu, color: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let layout = Vertex::layout();
        let vertex = vec![VertexBufferKey { stride: layout.array_stride, step: layout.step_mode, attributes: layout.attributes.to_vec() }];
        self.pipeline_for(gpu, &PipelineKey { color: Some(color), depth: Some(DEPTH_FORMAT), vertex, state: RasterState::default() })
    }

    /// The pipeline for `key`, built on first use.
    pub fn pipeline_for(&self, gpu: &Gpu, key: &PipelineKey) -> wgpu::RenderPipeline {
        let mut cache = self.pipelines.lock().unwrap();
        if let Some(p) = cache.get(key) {
            return p.clone();
        }
        let p = self.build(gpu, key);
        cache.insert(key.clone(), p.clone());
        p
    }

    /// The pipeline for `key`, or the validation error building it. A failed key is remembered,
    /// so draws that fall back on it don't rebuild it every frame.
    pub fn try_pipeline(&self, gpu: &Gpu, key: &PipelineKey) -> Result<wgpu::RenderPipeline, String> {
        if let Some(e) = self.failed.lock().unwrap().get(key) {
            return Err(e.clone());
        }
        if let Some(p) = self.pipelines.lock().unwrap().get(key) {
            return Ok(p.clone());
        }
        match gpu.checked(|| self.build(gpu, key)) {
            Ok(p) => {
                self.pipelines.lock().unwrap().insert(key.clone(), p.clone());
                Ok(p)
            }
            Err(e) => {
                self.failed.lock().unwrap().insert(key.clone(), e.clone());
                Err(e)
            }
        }
    }

    fn build(&self, gpu: &Gpu, key: &PipelineKey) -> wgpu::RenderPipeline {
        let s = &key.state;
        let buffers: Vec<_> = key.vertex.iter().map(|b| Some(b.as_layout())).collect();
        let writes = if s.color_writes { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() };
        let targets = [key.color.map(|format| wgpu::ColorTargetState { format, blend: s.blend, write_mask: writes })];
        gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&self.label),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState { module: &self.vertex.module, entry_point: Some(&self.vertex.entry), compilation_options: Default::default(), buffers: &buffers },
            primitive: wgpu::PrimitiveState { topology: s.topology, cull_mode: s.cull, ..Default::default() },
            depth_stencil: key.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(s.depth_write),
                depth_compare: Some(s.depth_compare),
                stencil: Default::default(),
                bias: s.bias_state(),
            }),
            multisample: Default::default(),
            fragment: self.fragment.as_ref().filter(|_| key.color.is_some()).map(|f| wgpu::FragmentState {
                module: &f.module,
                entry_point: Some(&f.entry),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}
