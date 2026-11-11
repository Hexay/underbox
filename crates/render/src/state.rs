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
    pub color_writes: bool,
    /// Rasterizer depth bias: constant, and the slope scale as `f32` bits (keys must hash).
    pub depth_bias: (i32, u32),
}

impl RasterState {
    pub fn bias_state(&self) -> wgpu::DepthBiasState {
        wgpu::DepthBiasState { constant: self.depth_bias.0, slope_scale: f32::from_bits(self.depth_bias.1), clamp: 0.0 }
    }
}

impl Default for RasterState {
    fn default() -> Self {
        RasterState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull: None,
            depth_write: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: None,
            color_writes: true,
            depth_bias: (0, 0),
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
