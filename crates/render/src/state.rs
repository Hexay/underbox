//! Fixed-function state a pipeline is built for: vertex buffer layouts, raster, depth and blend.
//! Everything here is hashable so [`Program`](crate::Program) can cache one pipeline per key.

/// One vertex buffer slot. A stride of 0 repeats the first element for every vertex, which is
/// how attributes a mesh lacks are fed from a constant defaults buffer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexBufferKey {
    pub stride: u64,
    pub step: wgpu::VertexStepMode,
    pub attributes: Vec<wgpu::VertexAttribute>,
}

impl VertexBufferKey {
    pub fn as_layout(&self) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout { array_stride: self.stride, step_mode: self.step, attributes: &self.attributes }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterState {
    pub topology: wgpu::PrimitiveTopology,
    pub cull: Option<wgpu::Face>,
    pub depth_write: bool,
    pub depth_compare: wgpu::CompareFunction,
    pub blend: Option<wgpu::BlendState>,
    pub write_mask: wgpu::ColorWrites,
    /// Rasterizer depth bias: constant, and the slope scale as `f32` bits (keys must hash).
    pub depth_bias: (i32, u32),
    /// Ignored on depth formats without a stencil aspect.
    pub stencil: Option<Stencil>,
    /// Only with a multisampled target; single-sample pipelines leave it off (wgpu rejects it).
    pub alpha_to_coverage: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stencil {
    pub front: wgpu::StencilFaceState,
    pub back: wgpu::StencilFaceState,
    pub read_mask: u32,
    pub write_mask: u32,
    /// Set on the pass per draw.
    pub reference: u32,
}

impl Default for Stencil {
    fn default() -> Self {
        Stencil { front: wgpu::StencilFaceState::IGNORE, back: wgpu::StencilFaceState::IGNORE, read_mask: 0xFF, write_mask: 0xFF, reference: 0 }
    }
}

impl RasterState {
    pub fn bias_state(&self) -> wgpu::DepthBiasState {
        wgpu::DepthBiasState { constant: self.depth_bias.0, slope_scale: f32::from_bits(self.depth_bias.1), clamp: 0.0 }
    }

    /// The stencil state for a depth target of `format`.
    pub fn stencil_state(&self, format: wgpu::TextureFormat) -> wgpu::StencilState {
        match self.stencil.filter(|_| format.has_stencil_aspect()) {
            Some(s) => wgpu::StencilState { front: s.front, back: s.back, read_mask: s.read_mask, write_mask: s.write_mask },
            None => wgpu::StencilState::default(),
        }
    }
}

impl Default for RasterState {
    fn default() -> Self {
        RasterState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull: None,
            depth_write: true,
            // Reverse Z, as native: near is 1, depth clears to 0.
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
            depth_bias: (0, 0),
            stencil: None,
            alpha_to_coverage: false,
        }
    }
}

/// Everything a render pipeline depends on besides its program.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PipelineKey {
    pub color: Option<wgpu::TextureFormat>,
    pub depth: Option<wgpu::TextureFormat>,
    pub vertex: Vec<VertexBufferKey>,
    pub state: RasterState,
}
