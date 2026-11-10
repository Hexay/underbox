//! vfx `.shader` parsing and translation: HLSL → DXC → SPIR-V → naga → WGSL, lazily per
//! (static, dynamic) combo with a disk cache. See research/shaders.md.

pub mod annotation;
pub mod blocks;
pub mod cache;
pub mod combo;
mod compat;
pub mod compiler;
pub mod decl;
pub mod dxc;
mod formats;
pub mod include;
pub mod reflect;
mod scan;
pub mod shader;
pub mod spirv;
pub mod translate;

pub use cache::Cache;
pub use combo::{Combo, ComboSet, INVALID_INDEX};
pub use compiler::{CompiledStage, Compiler, Variant};
pub use include::IncludePaths;
pub use reflect::{Reflection, bind_group_layouts};
pub use shader::{Program, Stage, VfxShader};
pub use translate::{Error, Phase};
