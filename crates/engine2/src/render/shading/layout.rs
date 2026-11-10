//! Member layout of a translated program's uniform blocks, read back from its WGSL with naga
//! (`shaders::Reflection` stops at bindings), and writing values into a block by member name.

use glam::{Mat4, Vec4};
use shaders::naga::{self, ScalarKind, TypeInner, VectorSize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Components of a float, int or uint scalar/vector (bools are uints in SPIR-V-derived WGSL).
    Float(u32),
    Int(u32),
    Uint(u32),
    Mat4,
    Other,
}

#[derive(Clone, Debug)]
pub struct Field {
    /// The HLSL name: naga appends `_` to names ending in a digit (`g_vInvProjRow3_`).
    pub name: String,
    pub offset: u32,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub size: u32,
    pub fields: Vec<Field>,
}

/// Uniform blocks by (group, binding).
pub type Blocks = HashMap<(u32, u32), Block>;

/// What a block member gets.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    Vec(Vec4),
    Mat(Mat4),
}

fn hlsl_name(name: &str) -> String {
    let b = name.as_bytes();
    if b.len() >= 2 && b[b.len() - 1] == b'_' && b[b.len() - 2].is_ascii_digit() { name[..name.len() - 1].to_owned() } else { name.to_owned() }
}

fn components(size: Option<VectorSize>) -> u32 {
    size.map_or(1, |s| s as u32)
}

fn kind(inner: &TypeInner) -> Kind {
    let (scalar, n) = match inner {
        TypeInner::Scalar(s) => (*s, 1),
        TypeInner::Vector { size, scalar } => (*scalar, components(Some(*size))),
        TypeInner::Matrix { columns: VectorSize::Quad, rows: VectorSize::Quad, scalar } if scalar.width == 4 => return Kind::Mat4,
        _ => return Kind::Other,
    };
    match (scalar.kind, scalar.width) {
        (ScalarKind::Float, 4) => Kind::Float(n),
        (ScalarKind::Sint, 4) => Kind::Int(n),
        (ScalarKind::Uint | ScalarKind::Bool, 4) => Kind::Uint(n),
        _ => Kind::Other,
    }
}

pub fn uniform_blocks(wgsl: &str) -> Blocks {
    let Ok(module) = naga::front::wgsl::parse_str(wgsl) else { return Blocks::new() };
    let mut out = Blocks::new();
    for (_, gv) in module.global_variables.iter() {
        let (naga::AddressSpace::Uniform, Some(rb)) = (gv.space, &gv.binding) else { continue };
        let TypeInner::Struct { members, span } = &module.types[gv.ty].inner else { continue };
        let fields = members
            .iter()
            .map(|m| Field { name: hlsl_name(m.name.as_deref().unwrap_or_default()), offset: m.offset, kind: kind(&module.types[m.ty].inner) })
            .collect();
        out.insert((rb.group, rb.binding), Block { size: *span, fields });
    }
    out
}

impl Block {
    /// The block's bytes, each member from `value(name)` (zero when it answers `None`).
    pub fn fill(&self, value: impl Fn(&str) -> Option<Value>) -> Vec<u8> {
        let mut bytes = vec![0u8; self.size.max(16) as usize];
        for f in &self.fields {
            let Some(v) = value(&f.name) else { continue };
            let words: Vec<u32> = match (f.kind, v) {
                (Kind::Mat4, Value::Mat(m)) => m.to_cols_array().map(f32::to_bits).to_vec(),
                (Kind::Float(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| x.to_bits()).collect(),
                (Kind::Int(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| (*x as i32) as u32).collect(),
                (Kind::Uint(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| x.max(0.0) as u32).collect(),
                _ => continue,
            };
            let at = f.offset as usize;
            for (i, w) in words.iter().enumerate() {
                if let Some(dst) = bytes.get_mut(at + i * 4..at + i * 4 + 4) {
                    dst.copy_from_slice(&w.to_le_bytes());
                }
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WGSL: &str = "struct B { m: mat4x4<f32>, v3_: vec3<f32>, f: f32, b: u32, i: vec2<i32> }\n\
        @group(0) @binding(4) var<uniform> blk: B;\n\
        @vertex fn vs() -> @builtin(position) vec4<f32> { return blk.m[0] * f32(blk.b); }";

    #[test]
    fn reads_members_and_fills_by_name() {
        let blocks = uniform_blocks(WGSL);
        let b = &blocks[&(0, 4)];
        let names: Vec<_> = b.fields.iter().map(|f| (f.name.as_str(), f.offset, f.kind)).collect();
        assert_eq!(names, [("m", 0, Kind::Mat4), ("v3", 64, Kind::Float(3)), ("f", 76, Kind::Float(1)), ("b", 80, Kind::Uint(1)), ("i", 88, Kind::Int(2))]);
        let bytes = b.fill(|n| match n {
            "v3" => Some(Value::Vec(Vec4::new(1.0, 2.0, 3.0, 9.0))),
            "b" => Some(Value::Vec(Vec4::ONE)),
            "i" => Some(Value::Vec(Vec4::new(-2.0, 5.0, 0.0, 0.0))),
            _ => None,
        });
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!([f32::from_bits(word(64)), f32::from_bits(word(72)), f32::from_bits(word(76))], [1.0, 3.0, 0.0]);
        assert_eq!((word(80), word(88) as i32, word(92)), (1, -2, 5));
    }
}
