//! Member layout of a translated program's uniform blocks and structured-buffer elements, read
//! back from its WGSL with naga (`shaders::Reflection` stops at bindings), and writing values into
//! them by member name.

use glam::{Mat4, Vec4};
use shaders::naga::{self, ArraySize, ScalarKind, TypeInner, VectorSize};
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

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The HLSL name: naga appends `_` to names ending in a digit (`g_vInvProjRow3_`).
    pub name: String,
    pub offset: u32,
    pub kind: Kind,
    /// Elements of a fixed-size array member (1 otherwise), `stride` bytes apart.
    pub count: u32,
    pub stride: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub size: u32,
    pub fields: Vec<Field>,
}

/// Uniform blocks by (group, binding).
pub type Blocks = HashMap<(u32, u32), Block>;

/// What a block member gets; an `Array` fills an array member element by element.
#[derive(Clone, Debug)]
pub enum Value {
    Vec(Vec4),
    Mat(Mat4),
    Array(Vec<Value>),
}

fn hlsl_name(name: &str) -> String {
    let b = name.as_bytes();
    if b.len() >= 2 && b[b.len() - 1] == b'_' && b[b.len() - 2].is_ascii_digit() { name[..name.len() - 1].to_owned() } else { name.to_owned() }
}

fn kind(inner: &TypeInner) -> Kind {
    let (scalar, n) = match inner {
        TypeInner::Scalar(s) => (*s, 1),
        TypeInner::Vector { size, scalar } => (*scalar, *size as u32),
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

fn field(module: &naga::Module, name: &str, offset: u32, ty: naga::Handle<naga::Type>) -> Field {
    let inner = &module.types[ty].inner;
    let (kind, count, stride) = match inner {
        TypeInner::Array { base, size: ArraySize::Constant(n), stride } => (self::kind(&module.types[*base].inner), n.get(), *stride),
        other => (self::kind(other), 1, 0),
    };
    Field { name: hlsl_name(name), offset, kind, count, stride }
}

fn block(module: &naga::Module, ty: naga::Handle<naga::Type>) -> Option<Block> {
    match &module.types[ty].inner {
        TypeInner::Struct { members, span } => {
            let fields = members.iter().map(|m| field(module, m.name.as_deref().unwrap_or_default(), m.offset, m.ty)).collect();
            Some(Block { size: *span, fields })
        }
        other => Some(Block { size: other.size(module.to_ctx()), fields: vec![field(module, "", 0, ty)] }),
    }
}

pub fn uniform_blocks(wgsl: &str) -> Blocks {
    let Ok(module) = naga::front::wgsl::parse_str(wgsl) else { return Blocks::new() };
    let mut out = Blocks::new();
    for (_, gv) in module.global_variables.iter() {
        let (naga::AddressSpace::Uniform, Some(rb)) = (gv.space, &gv.binding) else { continue };
        if let TypeInner::Struct { .. } = module.types[gv.ty].inner {
            out.extend(block(&module, gv.ty).map(|b| ((rb.group, rb.binding), b)));
        }
    }
    out
}

/// The element of each runtime-sized storage buffer (`StructuredBuffer<T>`), by (group, binding):
/// its fields, and `size` = the array stride. A scalar or vector element is one unnamed field.
pub fn storage_elements(wgsl: &str) -> Blocks {
    let Ok(module) = naga::front::wgsl::parse_str(wgsl) else { return Blocks::new() };
    let mut out = Blocks::new();
    for (_, gv) in module.global_variables.iter() {
        let (naga::AddressSpace::Storage { .. }, Some(rb)) = (gv.space, &gv.binding) else { continue };
        // DXC wraps the array in a one-member struct.
        let array = match &module.types[gv.ty].inner {
            TypeInner::Struct { members, .. } => members.last().map(|m| m.ty),
            _ => Some(gv.ty),
        };
        let Some(TypeInner::Array { base, size: ArraySize::Dynamic, stride }) = array.map(|a| &module.types[a].inner) else { continue };
        if let Some(element) = block(&module, *base) {
            out.insert((rb.group, rb.binding), Block { size: *stride, ..element });
        }
    }
    out
}

fn words(kind: Kind, v: &Value) -> Option<Vec<u32>> {
    Some(match (kind, v) {
        (Kind::Mat4, Value::Mat(m)) => m.to_cols_array().map(f32::to_bits).to_vec(),
        (Kind::Float(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| x.to_bits()).collect(),
        (Kind::Int(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| (*x as i32) as u32).collect(),
        (Kind::Uint(n), Value::Vec(v)) => v.to_array()[..n as usize].iter().map(|x| x.max(0.0) as u32).collect(),
        _ => return None,
    })
}

fn put(bytes: &mut [u8], at: usize, words: &[u32]) {
    for (i, w) in words.iter().enumerate() {
        if let Some(dst) = bytes.get_mut(at + i * 4..at + i * 4 + 4) {
            dst.copy_from_slice(&w.to_le_bytes());
        }
    }
}

impl Block {
    fn write(&self, bytes: &mut [u8], base: usize, value: impl Fn(&str) -> Option<Value>) {
        for f in &self.fields {
            let Some(v) = value(&f.name) else { continue };
            let elements = match &v {
                Value::Array(items) => items.iter().take(f.count as usize).collect(),
                single => vec![single],
            };
            for (i, e) in elements.into_iter().enumerate() {
                if let Some(w) = words(f.kind, e) {
                    put(bytes, base + f.offset as usize + i * f.stride as usize, &w);
                }
            }
        }
    }

    /// The block's bytes, each member from `value(name)` (zero when it answers `None`).
    pub fn fill(&self, value: impl Fn(&str) -> Option<Value>) -> Vec<u8> {
        let mut bytes = vec![0u8; self.size.max(16) as usize];
        self.write(&mut bytes, 0, value);
        bytes
    }

    /// `count` elements of a storage buffer whose element this is (at least one, zeroed).
    pub fn fill_elements(&self, count: usize, value: impl Fn(usize, &str) -> Option<Value>) -> Vec<u8> {
        let stride = self.size.max(4) as usize;
        let mut bytes = vec![0u8; stride * count.max(1)];
        for i in 0..count {
            self.write(&mut bytes, i * stride, |n| value(i, n));
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WGSL: &str = "struct B { m: mat4x4<f32>, v3_: vec3<f32>, f: f32, b: u32, i: vec2<i32>, a: array<vec4<f32>, 2> }\n\
        struct L { k: u32, c: vec3<f32> }\n\
        struct S { e: array<L> }\n\
        @group(0) @binding(4) var<uniform> blk: B;\n\
        @group(0) @binding(5) var<storage> lights: S;\n\
        @vertex fn vs() -> @builtin(position) vec4<f32> { return blk.m[0] * f32(blk.b) + blk.a[1] * lights.e[0].c.x; }";

    #[test]
    fn reads_members_and_fills_by_name() {
        let blocks = uniform_blocks(WGSL);
        let b = &blocks[&(0, 4)];
        let names: Vec<_> = b.fields.iter().map(|f| (f.name.as_str(), f.offset, f.kind, f.count)).collect();
        assert_eq!(names, [("m", 0, Kind::Mat4, 1), ("v3", 64, Kind::Float(3), 1), ("f", 76, Kind::Float(1), 1), ("b", 80, Kind::Uint(1), 1), ("i", 88, Kind::Int(2), 1), ("a", 96, Kind::Float(4), 2)]);
        let bytes = b.fill(|n| match n {
            "v3" => Some(Value::Vec(Vec4::new(1.0, 2.0, 3.0, 9.0))),
            "b" => Some(Value::Vec(Vec4::ONE)),
            "i" => Some(Value::Vec(Vec4::new(-2.0, 5.0, 0.0, 0.0))),
            "a" => Some(Value::Array(vec![Value::Vec(Vec4::ZERO), Value::Vec(Vec4::splat(7.0))])),
            _ => None,
        });
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!([f32::from_bits(word(64)), f32::from_bits(word(72)), f32::from_bits(word(76))], [1.0, 3.0, 0.0]);
        assert_eq!((word(80), word(88) as i32, word(92)), (1, -2, 5));
        assert_eq!(f32::from_bits(word(112)), 7.0);
    }

    #[test]
    fn storage_elements_fill_per_index() {
        let e = &storage_elements(WGSL)[&(0, 5)];
        assert_eq!((e.size, e.fields.len()), (32, 2));
        let bytes = e.fill_elements(2, |i, n| (n == "k").then(|| Value::Vec(Vec4::splat(i as f32 + 1.0))));
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!((bytes.len(), word(0), word(32)), (64, 1, 2));
    }
}
