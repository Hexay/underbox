//! naga types → wgpu formats for reflection, and storage images retargeted to another format.

use naga::common::wgsl::ToWgsl;
use naga::{ScalarKind, StorageFormat as S, TypeInner, VectorSize};
use wgpu_types::{TextureFormat as T, VertexFormat as V};

const STORAGE: [(S, T); 41] = [
    (S::R8Unorm, T::R8Unorm),
    (S::R8Snorm, T::R8Snorm),
    (S::R8Uint, T::R8Uint),
    (S::R8Sint, T::R8Sint),
    (S::R16Uint, T::R16Uint),
    (S::R16Sint, T::R16Sint),
    (S::R16Float, T::R16Float),
    (S::Rg8Unorm, T::Rg8Unorm),
    (S::Rg8Snorm, T::Rg8Snorm),
    (S::Rg8Uint, T::Rg8Uint),
    (S::Rg8Sint, T::Rg8Sint),
    (S::R32Uint, T::R32Uint),
    (S::R32Sint, T::R32Sint),
    (S::R32Float, T::R32Float),
    (S::Rg16Uint, T::Rg16Uint),
    (S::Rg16Sint, T::Rg16Sint),
    (S::Rg16Float, T::Rg16Float),
    (S::Rgba8Unorm, T::Rgba8Unorm),
    (S::Rgba8Snorm, T::Rgba8Snorm),
    (S::Rgba8Uint, T::Rgba8Uint),
    (S::Rgba8Sint, T::Rgba8Sint),
    (S::Bgra8Unorm, T::Bgra8Unorm),
    (S::Rgb10a2Uint, T::Rgb10a2Uint),
    (S::Rgb10a2Unorm, T::Rgb10a2Unorm),
    (S::Rg11b10Ufloat, T::Rg11b10Ufloat),
    (S::R64Uint, T::R64Uint),
    (S::Rg32Uint, T::Rg32Uint),
    (S::Rg32Sint, T::Rg32Sint),
    (S::Rg32Float, T::Rg32Float),
    (S::Rgba16Uint, T::Rgba16Uint),
    (S::Rgba16Sint, T::Rgba16Sint),
    (S::Rgba16Float, T::Rgba16Float),
    (S::Rgba32Uint, T::Rgba32Uint),
    (S::Rgba32Sint, T::Rgba32Sint),
    (S::Rgba32Float, T::Rgba32Float),
    (S::R16Unorm, T::R16Unorm),
    (S::R16Snorm, T::R16Snorm),
    (S::Rg16Unorm, T::Rg16Unorm),
    (S::Rg16Snorm, T::Rg16Snorm),
    (S::Rgba16Unorm, T::Rgba16Unorm),
    (S::Rgba16Snorm, T::Rgba16Snorm),
];

pub fn storage_format(f: S) -> T {
    STORAGE.iter().find(|(s, _)| *s == f).map(|(_, t)| *t).expect("every naga storage format is listed")
}

/// Rewrites the storage image at (`group`, `binding`) to `format`: its WGSL declaration and its
/// reflected binding. DXC infers `RWTexture2D<float4>` as rgba32f whatever managed binds, and a
/// WGSL `textureStore` takes a vec4 for every format, so only the declaration changes.
pub fn retarget_storage(stage: &mut crate::CompiledStage, group: u32, binding: u32, format: T) -> Result<(), String> {
    let name = STORAGE.iter().find(|(_, t)| *t == format).map(|(s, _)| s.to_wgsl()).ok_or_else(|| format!("{format:?} isn't a storage format"))?;
    let at = format!("@group({group}) @binding({binding})");
    let start = stage.wgsl.find(&at).ok_or_else(|| format!("no WGSL declaration at {at}"))?;
    // naga may put the attributes and the `var` on separate lines.
    let decl = &stage.wgsl[start..start + stage.wgsl[start..].find(';').ok_or_else(|| format!("{at}: unterminated"))?];
    let open = decl.find("texture_storage_").and_then(|i| decl[i..].find('<').map(|j| start + i + j + 1)).ok_or_else(|| format!("{at} isn't a storage image"))?;
    let close = open + stage.wgsl[open..].find(',').ok_or_else(|| format!("{at}: no access mode"))?;
    stage.wgsl.replace_range(open..close, name);
    for b in stage.reflection.bindings.iter_mut().filter(|b| (b.group, b.binding) == (group, binding)) {
        if let wgpu_types::BindingType::StorageTexture { format: f, .. } = &mut b.ty {
            *f = format;
        }
    }
    Ok(())
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
