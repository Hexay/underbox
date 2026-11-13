//! A material program's vertex inputs fed from a mesh layout, matched by HLSL semantic
//! (`TEXCOORD0` ↔ texcoord 0). Slot 0 is the mesh; slot 1 the per-instance transform ids
//! (`TEXCOORD13`, `InstanceTransformUv`), which index the draw's transform buffer from 0; then one
//! per-vertex index stream for each `MorphIndex` input the mesh lacks (`TEXCOORD14`/`15`: vertex
//! `i` reads `i`, so a vertex-cache draw finds vertex offset + i); last a constant defaults buffer
//! (stride 0, per instance) for the other inputs the mesh lacks. The slots are real's: its capture
//! binds the same, in this order.

use crate::mesh::layout::{Layout, format};
use render::VertexBufferKey;
use render::wgpu::{VertexAttribute, VertexFormat as F, VertexStepMode};
use shaders::reflect::VertexInput;

/// Defaults buffer: white at 0 (colours), +Z at 16 (normals), zeros from 32.
pub const DEFAULTS: [f32; 16] = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
const INSTANCE_ID: (&str, i32) = ("texcoord", 13);
const INDEX_STREAMS: [(&str, i32); 2] = [("texcoord", 14), ("texcoord", 15)];

/// What feeds a vertex buffer slot after the mesh's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    InstanceIds,
    /// Floats 0, 1, 2, ... per vertex.
    VertexIndex,
    Defaults,
}

/// The stream behind each of `keys` after the first (the mesh), as [`inputs`] lays them out.
pub fn streams(keys: &[VertexBufferKey]) -> Vec<Stream> {
    keys.iter()
        .skip(1)
        .map(|k| match (k.step, k.stride) {
            (_, 0) => Stream::Defaults,
            (VertexStepMode::Vertex, _) => Stream::VertexIndex,
            _ => Stream::InstanceIds,
        })
        .collect()
}

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
        format::RG16F => F::Float16x2,
        format::RGB10A2_UNORM => F::Unorm10_10_10_2,
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
    let (mut mesh, mut defaults, mut instance, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut uncompressed_normals = false;
    for input in shader {
        let want = input.format?;
        let (name, index) = semantic(&input.semantic);
        let field = layout.fields.iter().find(|f| f.semantic == name && f.index == index && f.offset % 4 == 0);
        let found = field.and_then(|f| Some((f, mesh_format(f.format)?))).filter(|(_, fmt)| base(*fmt) == base(want));
        let attr = |format, offset| VertexAttribute { format, offset, shader_location: input.location };
        match found {
            Some((f, fmt)) => {
                uncompressed_normals |= name == "normal" && base(fmt) == Base::Float && !matches!(fmt, F::Unorm8x4 | F::Unorm10_10_10_2);
                mesh.push(attr(fmt, f.offset as u64));
            }
            None if (name.as_str(), index) == INSTANCE_ID && base(want) == Base::Uint => instance.push(attr(F::Uint32, 0)),
            None if INDEX_STREAMS.contains(&(name.as_str(), index)) && want == F::Float32 => indices.push((index, attr(F::Float32, 0))),
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
    indices.sort_by_key(|(index, _)| *index);
    let mut keys = vec![VertexBufferKey { stride, step: VertexStepMode::Vertex, attributes: mesh }, VertexBufferKey { stride: 4, step: VertexStepMode::Instance, attributes: instance }];
    keys.extend(indices.into_iter().map(|(_, a)| VertexBufferKey { stride: 4, step: VertexStepMode::Vertex, attributes: vec![a] }));
    keys.push(VertexBufferKey { stride: 0, step: VertexStepMode::Instance, attributes: defaults });
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
        assert_eq!(locations(&i.keys[1]), [2]);
        assert_eq!(streams(&i.keys), [Stream::InstanceIds, Stream::Defaults]);
        assert_eq!(semantic("TEXCOORD13"), ("texcoord".into(), 13));
        assert_eq!(semantic("POSITION"), ("position".into(), 0));
    }

    #[test]
    fn vertex_cache_inputs_get_index_streams_in_real_slots() {
        let layout = Layout::skinned_draw_vertex();
        let shader = [input(0, "POSITION", F::Float32x3), input(1, "TEXCOORD15", F::Float32), input(2, "TEXCOORD14", F::Float32), input(3, "TEXCOORD13", F::Uint32), input(4, "BLENDINDICES", F::Uint32x4)];
        let i = inputs(&shader, &layout).unwrap();
        assert_eq!(streams(&i.keys), [Stream::InstanceIds, Stream::VertexIndex, Stream::VertexIndex, Stream::Defaults]);
        assert_eq!(i.keys[2].attributes[0].shader_location, 2);
        assert_eq!(i.keys[0].attributes.iter().map(|a| a.offset).collect::<Vec<_>>(), [0, 60]);
    }
}
