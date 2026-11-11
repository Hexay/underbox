//! A compute program: one stage, its bind-group layouts and pipeline, and dispatch by thread
//! count the way s&box's `RenderTools.Compute` takes it.

use crate::{Gpu, Stage};

pub struct ComputeProgram {
    pub label: String,
    pub bind_group_layouts: Vec<wgpu::BindGroupLayout>,
    pub workgroup_size: [u32; 3],
    pipeline: wgpu::ComputePipeline,
}

impl ComputeProgram {
    pub fn new(gpu: &Gpu, label: &str, stage: &Stage, bind_group_layouts: Vec<wgpu::BindGroupLayout>, workgroup_size: [u32; 3]) -> ComputeProgram {
        let refs: Vec<_> = bind_group_layouts.iter().map(Some).collect();
        let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &refs, immediate_size: 0 });
        let pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            module: &stage.module,
            entry_point: Some(&stage.entry),
            compilation_options: Default::default(),
            cache: None,
        });
        ComputeProgram { label: label.into(), bind_group_layouts, workgroup_size, pipeline }
    }

    /// Workgroups covering `threads` (each axis rounded up to whole groups, at least one).
    pub fn groups(&self, threads: [u32; 3]) -> [u32; 3] {
        std::array::from_fn(|i| threads[i].max(1).div_ceil(self.workgroup_size[i].max(1)))
    }

    /// Run over `threads` and submit.
    pub fn dispatch(&self, gpu: &Gpu, bind_groups: &[wgpu::BindGroup], threads: [u32; 3]) {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(&self.label) });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(&self.label), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            for (i, g) in bind_groups.iter().enumerate() {
                pass.set_bind_group(i as u32, g, &[]);
            }
            let [x, y, z] = self.groups(threads);
            pass.dispatch_workgroups(x, y, z);
        }
        gpu.submit(encoder.finish());
    }
}
