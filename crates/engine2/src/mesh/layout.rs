//! Vertex layouts: managed's `VertexLayout` objects (built from `[VertexLayout.*]` attributes) and
//! the legacy `VertexField` lists, plus decoding a vertex of any layout into [`Vertex`].
//! The renderer reads the same [`Layout`] to build its vertex state.

use super::vertex::{Vertex, stream};
use abi::slots::vertex_layout as vl;
use abi::{NativeRef, Registry, bind, class};
use std::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::VertexLayout>;

/// managed's `ColorFormat` values vertex attributes use (Core/Interop/NativeEngine/ColorFormat.cs).
pub mod format {
    pub const RGBA32F: u32 = 2;
    pub const RGB32F: u32 = 6;
    pub const RG32F: u32 = 16;
    pub const RGBA8_UNORM: u32 = 28;
    pub const RGBA8_UINT: u32 = 30;
    pub const R32F: u32 = 41;
    pub const R32_UINT: u32 = 42;
    pub const R32_SINT: u32 = 43;
    pub const R8_UINT: u32 = 62;
    pub const R8_SINT: u32 = 64;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Lower-case semantic: position, normal, tangent, texcoord, color, blendweight, blendindices.
    pub semantic: String,
    pub index: i32,
    pub format: u32,
    pub offset: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub name: String,
    pub stride: usize,
    pub fields: Vec<Field>,
}

/// Bytes one attribute of `format` takes; 0 for formats vertices don't use.
pub fn format_size(format: u32) -> usize {
    match format {
        format::RGBA32F => 16,
        format::RGB32F => 12,
        format::RG32F => 8,
        format::RGBA8_UNORM | format::RGBA8_UINT | format::R32F | format::R32_UINT | format::R32_SINT => 4,
        format::R8_UINT | format::R8_SINT => 1,
        _ => 0,
    }
}

fn read_f32(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}

/// An attribute's components as floats (unorm bytes scaled to 0..1); missing ones are 0.
fn components(bytes: &[u8], f: &Field) -> [f32; 4] {
    let mut out = [0.0; 4];
    let at = f.offset;
    match f.format {
        format::RGBA32F | format::RGB32F | format::RG32F | format::R32F => {
            let n = format_size(f.format) / 4;
            (0..n).for_each(|i| out[i] = read_f32(bytes, at + i * 4));
        }
        format::RGBA8_UNORM | format::RGBA8_UINT => {
            let scale = if f.format == format::RGBA8_UNORM { 1.0 / 255.0 } else { 1.0 };
            (0..4).for_each(|i| out[i] = f32::from(bytes.get(at + i).copied().unwrap_or(0)) * scale);
        }
        format::R32_UINT | format::R32_SINT => out[0] = bytes.get(at..at + 4).map_or(0.0, |s| i32::from_le_bytes(s.try_into().unwrap()) as f32),
        format::R8_UINT | format::R8_SINT => out[0] = f32::from(bytes.get(at).copied().unwrap_or(0)),
        _ => {}
    }
    out
}

impl Layout {
    /// The layout of [`Vertex`] itself, for meshes we fill (compiled `.vmdl` geometry).
    pub fn sbox_vertex() -> Arc<Layout> {
        let field = |semantic: &str, index, format, offset| Field { semantic: semantic.into(), index, format, offset };
        Arc::new(Layout {
            name: "Vertex".into(),
            stride: size_of::<Vertex>(),
            fields: vec![
                field("position", 0, format::RGB32F, 0),
                field("color", 0, format::RGBA8_UNORM, 12),
                field("normal", 0, format::RGB32F, 16),
                field("texcoord", 0, format::RGBA32F, 28),
                field("texcoord", 1, format::RGBA32F, 44),
                field("tangent", 0, format::RGBA32F, 60),
            ],
        })
    }

    /// [`Layout::sbox_vertex`] followed by four bone indices and four unorm weights (summing to
    /// 255), for compiled meshes with a skeleton: what skinning_cs reads (render/skin).
    pub fn sbox_skinned_vertex() -> Arc<Layout> {
        let base = Self::sbox_vertex();
        let mut fields = base.fields.clone();
        let at = base.stride;
        fields.push(Field { semantic: "blendindices".into(), index: 0, format: format::RGBA8_UINT, offset: at });
        fields.push(Field { semantic: "blendweight".into(), index: 0, format: format::RGBA8_UNORM, offset: at + 4 });
        Arc::new(Layout { name: "SkinnedVertex".into(), stride: at + 8, fields })
    }

    /// Which `VertexStreams` this layout carries.
    pub fn streams(&self) -> i32 {
        self.fields.iter().fold(0, |m, f| {
            m | match (f.semantic.as_str(), f.index) {
                ("position", 0) => stream::POSITION,
                ("normal", 0) => stream::NORMAL,
                ("tangent", 0) => stream::TANGENT,
                ("texcoord", 0) => stream::TEX0,
                ("texcoord", 1) => stream::TEX1,
                ("color", 0) => stream::COLOR,
                _ => 0,
            }
        })
    }

    /// One vertex (`bytes` is exactly its stride) as a [`Vertex`]; absent attributes keep defaults.
    pub fn decode(&self, bytes: &[u8]) -> Vertex {
        let mut v = Vertex::default();
        for f in &self.fields {
            let c = components(bytes, f);
            match (f.semantic.as_str(), f.index) {
                ("position", 0) => v.position = [c[0], c[1], c[2]],
                ("normal", 0) => v.normal = [c[0], c[1], c[2]],
                ("tangent", 0) => v.tangent = if format_size(f.format) == 16 { c } else { [c[0], c[1], c[2], -1.0] },
                ("texcoord", 0) => v.tex0 = c,
                ("texcoord", 1) => v.tex1 = c,
                ("color", 0) => v.color = c.map(|x| if f.format == format::RGBA8_UINT { x } else { x * 255.0 }.round().clamp(0.0, 255.0) as u8),
                _ => {}
            }
        }
        v
    }

    /// Every whole vertex in `bytes`.
    pub fn decode_all(&self, bytes: &[u8]) -> Vec<Vertex> {
        if self.stride == 0 {
            return Vec::new();
        }
        bytes.chunks_exact(self.stride).map(|b| self.decode(b)).collect()
    }
}

/// managed's `VertexField` (Mesh.VertexBuffer.cs): format, byte offset, name span, semantic index.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VertexField {
    pub format: i32,
    pub offset: i32,
    pub name_size: i32,
    pub name_offset: i32,
    pub semantic_index: i32,
}

/// A layout from the legacy field list; each field's name is a span of the concatenated `names`.
pub fn from_fields(stride: usize, names: &str, fields: &[VertexField]) -> Layout {
    let name_of = |f: &VertexField| {
        let start = usize::try_from(f.name_offset).unwrap_or(0);
        let end = start + usize::try_from(f.name_size).unwrap_or(0);
        names.get(start..end).unwrap_or_default().to_ascii_lowercase()
    };
    let fields = fields.iter().map(|f| Field { semantic: name_of(f), index: f.semantic_index, format: f.format as u32, offset: f.offset.max(0) as usize }).collect();
    Layout { name: String::new(), stride, fields }
}

pub fn get(r: Ref) -> Option<Arc<Layout>> {
    objects::get::<_, Mutex<Layout>>(r).map(|l| Arc::new(l.lock().unwrap().clone()))
}

fn with<R: Default>(p: *mut c_void, f: impl FnOnce(&mut Layout) -> R) -> R {
    objects::get::<_, Mutex<Layout>>(Ref::from_raw(p)).map(|l| f(&mut l.lock().unwrap())).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, vl::CREATE, |name: Option<&CStr>, size: i32| -> Ref {
        let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        objects::create(Mutex::new(Layout { name, stride: size.max(0) as usize, fields: Vec::new() }))
    });
    bind!(r, vl::DESTROY, |l| {
        objects::destroy(l);
    });
    bind!(r, vl::FREE, |l| {
        objects::destroy(l);
    });
    bind!(r, vl::ADD, |l: Ref, semantic: Option<&CStr>, index, format, offset: i32| {
        let semantic = semantic.map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        with(l.as_raw(), |layout| layout.fields.push(Field { semantic, index, format, offset: offset.max(0) as usize }));
    });
    bind!(r, vl::BUILD, |_| {});
    bind!(r, vl::GET_M_SIZE, |p| with(p, |l| l.stride as i32));
    bind!(r, vl::SET_M_SIZE, |p, v: i32| with(p, |l| l.stride = v.max(0) as usize));
}
