//! vfx `.shader` parsing and translation: HLSL → DXC → SPIR-V → naga → WGSL, lazily per
//! (static, dynamic) combo with a disk cache. See research/shaders.md.

pub mod annotation;
mod bindless_fetch;
pub mod bindings;
pub mod blocks;
mod builtin;
pub mod cache;
pub mod combo;
mod compat;
pub mod comparison;
pub mod compiler;
pub mod decl;
pub mod directives;
pub mod dxc;
pub mod expr;
mod formats;
mod image_atomics;
mod image_coords;
mod overrides;
pub mod include;
pub mod packed_buffers;
mod packed_layout;
pub mod reflect;
mod rematerialize;
pub mod row_major;
mod scan;
pub mod shader;
pub mod spirv;
pub mod translate;
mod word_buffers;

// hassle-rs's `com` dependency calls the registry API without linking it.
#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "C" {}

pub use cache::Cache;
pub use combo::{Combo, ComboSet, INVALID_INDEX};
pub use compiler::{CompiledStage, Compiler, Variant};
pub use formats::retarget_storage;
pub use image_atomics::TEXEL_WORDS;
pub use include::IncludePaths;
/// The naga the WGSL comes from, for consumers reading it back (uniform member layouts).
pub use naga;
pub use reflect::{Reflection, bind_group_layouts};
pub use shader::{Program, Stage, VfxShader};
pub use translate::{Error, Phase};
