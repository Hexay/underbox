//! naga types → wgpu formats for reflection.

use naga::{ScalarKind, StorageFormat as S, TypeInner, VectorSize};
use wgpu_types::{TextureFormat as T, VertexFormat as V};

pub fn storage_format(f: S) -> T {
    match f {
        S::R8Unorm => T::R8Unorm,
        S::R8Snorm => T::R8Snorm,
        S::R8Uint => T::R8Uint,
        S::R8Sint => T::R8Sint,
        S::R16Uint => T::R16Uint,
        S::R16Sint => T::R16Sint,
        S::R16Float => T::R16Float,
        S::Rg8Unorm => T::Rg8Unorm,
        S::Rg8Snorm => T::Rg8Snorm,
        S::Rg8Uint => T::Rg8Uint,
        S::Rg8Sint => T::Rg8Sint,
        S::R32Uint => T::R32Uint,
        S::R32Sint => T::R32Sint,
        S::R32Float => T::R32Float,
        S::Rg16Uint => T::Rg16Uint,
        S::Rg16Sint => T::Rg16Sint,
        S::Rg16Float => T::Rg16Float,
        S::Rgba8Unorm => T::Rgba8Unorm,
        S::Rgba8Snorm => T::Rgba8Snorm,
        S::Rgba8Uint => T::Rgba8Uint,
        S::Rgba8Sint => T::Rgba8Sint,
        S::Bgra8Unorm => T::Bgra8Unorm,
        S::Rgb10a2Uint => T::Rgb10a2Uint,
        S::Rgb10a2Unorm => T::Rgb10a2Unorm,
        S::Rg11b10Ufloat => T::Rg11b10Ufloat,
        S::R64Uint => T::R64Uint,
        S::Rg32Uint => T::Rg32Uint,
        S::Rg32Sint => T::Rg32Sint,
        S::Rg32Float => T::Rg32Float,
        S::Rgba16Uint => T::Rgba16Uint,
        S::Rgba16Sint => T::Rgba16Sint,
        S::Rgba16Float => T::Rgba16Float,
        S::Rgba32Uint => T::Rgba32Uint,
        S::Rgba32Sint => T::Rgba32Sint,
        S::Rgba32Float => T::Rgba32Float,
        S::R16Unorm => T::R16Unorm,
        S::R16Snorm => T::R16Snorm,
        S::Rg16Unorm => T::Rg16Unorm,
        S::Rg16Snorm => T::Rg16Snorm,
        S::Rgba16Unorm => T::Rgba16Unorm,
        S::Rgba16Snorm => T::Rgba16Snorm,
    }
}

/// The 32-bit vertex format matching a shader input's type (the buffer's real format comes from
/// the mesh layout; this is the shader side).
pub fn vertex_format(ty: &TypeInner) -> Option<V> {
    let (scalar, n) = match ty {
        TypeInner::Scalar(s) => (*s, 1),
        TypeInner::Vector { size, scalar } => (*scalar, match size {
            VectorSize::Bi => 2,
            VectorSize::Tri => 3,
            VectorSize::Quad => 4,
        }),
        _ => return None,
    };
    if scalar.width != 4 {
        return None;
    }
    let table = match scalar.kind {
        ScalarKind::Float => [V::Float32, V::Float32x2, V::Float32x3, V::Float32x4],
        ScalarKind::Sint => [V::Sint32, V::Sint32x2, V::Sint32x3, V::Sint32x4],
        ScalarKind::Uint => [V::Uint32, V::Uint32x2, V::Uint32x3, V::Uint32x4],
        _ => return None,
    };
    Some(table[n - 1])
}
