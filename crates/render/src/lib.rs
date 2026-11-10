//! underbox's renderer on wgpu. See docs/renderer.md.
//!
//! - [`Gpu`]: the one device and queue, created headless (offscreen) or for a window later.
//! - [`Texture`], [`Buffer`]: GPU resources; render targets are textures with `RENDER_ATTACHMENT`.
//! - [`Program`]: a shader program and its bind-group layouts (placeholder WGSL until the
//!   translation pipeline supplies per-combo modules).
//! - [`ForwardPass`]: draws [`Mesh`]es into a colour + depth target.
//! - [`Frame`]: a read-back image with hashing and tolerant comparison, for frame-hash tests.

mod buffer;
mod forward;
mod frame;
mod gpu;
mod mesh;
mod program;
mod texture;

pub use buffer::{Buffer, BufferKind};
pub use forward::{Camera, DrawItem, ForwardPass};
pub use frame::{Comparison, Frame};
pub use gpu::{Gpu, GpuError};
pub use mesh::{Mesh, Vertex};
pub use program::Program;
pub use texture::{DEPTH_FORMAT, Texture, TextureDesc};
pub use wgpu;
