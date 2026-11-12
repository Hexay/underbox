//! underbox's renderer on wgpu. See docs/renderer.md.
//!
//! - [`Gpu`]: the one device and queue, created headless (offscreen); [`Surface`] presents to a
//!   window when the window lane hands one over.
//! - [`Texture`], [`Buffer`]: GPU resources; render targets are textures with `RENDER_ATTACHMENT`.
//! - [`Program`]: a shader program (one module, or one per stage) and its bind-group layouts,
//!   with pipelines cached per [`PipelineKey`]; [`ComputeProgram`] for compute stages.
//! - [`Placeholder`] + [`pass::execute`]: the placeholder-shaded draw path every draw takes until
//!   translated combos exist; [`ForwardPass`] is the same for [`Mesh`]es.
//! - [`Frame`]: a read-back image with hashing and tolerant comparison, for frame-hash tests.

mod batch;
mod blit;
mod buffer;
mod compute;
mod forward;
mod frame;
mod gpu;
mod mesh;
pub mod pass;
mod placeholder;
mod program;
mod readback;
pub mod srgb;
mod state;
mod surface;
mod texture;
mod texture_io;

pub use batch::Slice;
pub use blit::{Blitter, Transfer};
pub use buffer::{Buffer, BufferKind};
pub use compute::ComputeProgram;
pub use forward::{Camera, DrawItem, ForwardPass};
pub use frame::{Comparison, Frame};
pub use gpu::{Gpu, GpuError, timed as gpu_timed};
pub use mesh::{Mesh, Vertex};
pub use placeholder::{DRAW_UNIFORM_SIZE, Placeholder, Semantic};
pub use program::{Program, Stage};
pub use state::{PipelineKey, RasterState, Stencil, VertexBufferKey};
pub use surface::{Surface, SurfaceError, SurfaceTarget};
pub use texture::{DEPTH_FORMAT, Texture, TextureDesc, srgb_twin};
pub use wgpu;
