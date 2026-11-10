//! A material program's vertex inputs fed from a mesh layout, matched by HLSL semantic
//! (`TEXCOORD0` ↔ texcoord 0). Slot 0 is the mesh; slot 1 a constant defaults buffer (stride 0)
//! for inputs the mesh lacks; slot 2 the per-instance transform ids (`TEXCOORD13`,
//! `InstanceTransformUv`), which index the draw's transform buffer from 0.

use crate::mesh::layout::{Layout, format};
use render::VertexBufferKey;
use render::wgpu::{VertexAttribute, VertexFormat as F, VertexStepMode};
use shaders::reflect::VertexInput;

/// Defaults buffer: white at 0 (colours), +Z at 16 (normals), zeros from 32.
pub const DEFAULTS: [f32; 16] = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
const INSTANCE_ID: (&str, i32) = ("texcoord", 13);

pub struct Inputs {
    pub keys: Vec<VertexBufferKey>,
    /// The mesh stores float normals, not the packed tangent frame (`UncompressedTangentFrame`).
    pub uncompressed_normals: bool,
}

fn mesh_format(f: u32) -> Option<F> {
    Some(match f {
        format::RGBA32F => F::Float32x4,
        format::RGB32F => F::Float32x3,
        format::RG32F => F::Float32x2,
        format::R32F => F::Float32,
        format::RGBA8_UNORM => F::Unorm8x4,
        format::RGBA8_UINT => F::Uint8x4,
        format::R32_UINT => F::Uint32,
        format::R32_SINT => F::Sint32,
        _ => return None,
    })
}

#[derive(PartialEq)]
enum Base {
    Float,
    Uint,
    Sint,
}

fn base(f: F) -> Base {
    let name = format!("{f:?}");
    if name.starts_with("Uint") {
        Base::Uint
    } else if name.starts_with("Sint") {
        Base::Sint
    } else {
        Base::Float
    }
}

/// `TEXCOORD13` → ("texcoord", 13); no digits → index 0.
fn semantic(s: &str) -> (String, i32) {
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (name, index) = s.split_at(s.len() - digits);
    (name.to_ascii_lowercase(), index.parse().unwrap_or(0))
}

/// `None` when an input has no known format or the layout can't feed it.
pub fn inputs(shader: &[VertexInput], layout: &Layout) -> Option<Inputs> {
    let (mut mesh, mut defaults, mut instance) = (Vec::new(), Vec::new(), Vec::new());
    let mut uncompressed_normals = false;
    for input in shader {
        let want = input.format?;
        let (name, index) = semantic(&input.semantic);
        let field = layout.fields.iter().find(|f| f.semantic == name && f.index == index && f.offset % 4 == 0);
        let found = field.and_then(|f| Some((f, mesh_format(f.format)?))).filter(|(_, fmt)| base(*fmt) == base(want));
        let attr = |format, offset| VertexAttribute { format, offset, shader_location: input.location };
        match found {
            Some((f, fmt)) => {
                uncompressed_normals |= name == "normal" && base(fmt) == Base::Float && fmt != F::Unorm8x4;
                mesh.push(attr(fmt, f.offset as u64));
            }
            None if (name.as_str(), index) == INSTANCE_ID && base(want) == Base::Uint => instance.push(attr(F::Uint32, 0)),
            None => {
                let offset = match (name.as_str(), base(want)) {
                    (_, Base::Uint | Base::Sint) => 32,
                    ("color", _) => 0,
                    ("normal", _) => 16,
                    _ => 32,
                };
                defaults.push(attr(want, offset));
            }
        }
    }
    let stride = layout.stride as u64;
    if stride == 0 || !stride.is_multiple_of(4) {
        return None;
    }
    let keys = vec![
        VertexBufferKey { stride, step: VertexStepMode::Vertex, attributes: mesh },
        VertexBufferKey { stride: 0, step: VertexStepMode::Vertex, attributes: defaults },
        VertexBufferKey { stride: 4, step: VertexStepMode::Instance, attributes: instance },
    ];
    Some(Inputs { keys, uncompressed_normals })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(location: u32, semantic: &str, format: F) -> VertexInput {
        VertexInput { location, semantic: semantic.into(), name: String::new(), format: Some(format) }
    }

    #[test]
    fn matches_semantics_and_fills_the_rest() {
        let layout = Layout::sbox_vertex();
        let shader = [input(0, "POSITION", F::Float32x3), input(1, "NORMAL", F::Float32x4), input(2, "TEXCOORD13", F::Uint32), input(3, "COLOR0", F::Float32x4)];
        let i = inputs(&shader, &layout).unwrap();
        let locations = |k: &VertexBufferKey| k.attributes.iter().map(|a| a.shader_location).collect::<Vec<_>>();
        assert!(locations(&i.keys[0]).contains(&0));
        assert_eq!(locations(&i.keys[2]), [2]);
        assert_eq!(semantic("TEXCOORD13"), ("texcoord".into(), 13));
        assert_eq!(semantic("POSITION"), ("position".into(), 0));
    }
}
