//! `CRenderAttributes`: named values shaders read (floats, vectors, textures, combos...), each
//! type in its own namespace as native keeps them, with a parent consulted for missing values.

use abi::slots::c_render_attributes as ra;
use abi::structs::{CSamplerStateDesc, Matrix, Vector2, Vector3, Vector4};
use abi::{NativeRef, Registry, StringToken, bind, class};
use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::CRenderAttributes>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Float,
    Vector2,
    Vector3,
    Vector4,
    Matrix,
    String,
    Int,
    Combo,
    Bool,
    Texture,
    Sampler,
    Buffer,
    Ptr,
    IntVector4,
    ConstantBuffer,
}

#[derive(Clone)]
pub enum Value {
    Float(f32),
    Vector2(Vector2),
    Vector3(Vector3),
    Vector4(Vector4),
    Matrix(Matrix),
    String(String),
    Int(i32),
    Combo(u8),
    Bool(bool),
    /// The texture's handle as managed passed it, and the mip it asked for.
    Texture(NativeRef<class::CTextureBase>, i32),
    Sampler(CSamplerStateDesc),
    Buffer(usize),
    Ptr(usize),
    IntVector4([i32; 4]),
    /// A whole constant block's bytes (`SetConstantBufferData`), by block name.
    ConstantBuffer(Arc<[u8]>),
}

#[derive(Default)]
pub struct RenderAttributes {
    values: Mutex<HashMap<(Kind, u32), Value>>,
    parent: Mutex<Option<Arc<RenderAttributes>>>,
}

impl RenderAttributes {
    fn set(&self, kind: Kind, name: StringToken, value: Value) {
        self.values.lock().unwrap().insert((kind, name.0), value);
    }

    fn get(&self, kind: Kind, name: StringToken) -> Option<Value> {
        if let Some(v) = self.values.lock().unwrap().get(&(kind, name.0)) {
            return Some(v.clone());
        }
        let parent = self.parent.lock().unwrap().clone();
        parent.and_then(|p| p.get(kind, name))
    }

    fn delete(&self, kind: Kind, name: StringToken) {
        self.values.lock().unwrap().remove(&(kind, name.0));
    }
}

/// A new, empty attributes object as managed holds it.
pub fn create() -> Ref {
    objects::create(RenderAttributes::default())
}

fn on<R: Default>(r: Ref, f: impl FnOnce(&RenderAttributes) -> R) -> R {
    objects::get::<_, RenderAttributes>(r).map(|a| f(&a)).unwrap_or_default()
}

/// A vector4 value (with parents), for native readers such as the camera's clear colour.
pub fn vector4(r: Ref, name: StringToken) -> Option<Vector4> {
    on(r, |a| match a.get(Kind::Vector4, name) {
        Some(Value::Vector4(v)) => Some(v),
        _ => None,
    })
}

pub fn int(r: Ref, name: StringToken) -> Option<i32> {
    on(r, |a| match a.get(Kind::Int, name) {
        Some(Value::Int(v)) => Some(v),
        _ => None,
    })
}

pub fn matrix(r: Ref, name: &str) -> Option<Matrix> {
    on(r, |a| match a.get(Kind::Matrix, StringToken::of(name)) {
        Some(Value::Matrix(m)) => Some(m),
        _ => None,
    })
}

/// A matrix value as a `float4x4` constant reads it: native uploads managed's row-major bytes
/// into column-major HLSL, so the shader's `mul( M, v )` applies managed's row-vector transform.
pub fn shader_matrix(r: Ref, name: &str) -> Option<glam::Mat4> {
    let n = matrix(r, name)?.numerics;
    Some(glam::Mat4::from_cols_array(&[n.m11, n.m12, n.m13, n.m14, n.m21, n.m22, n.m23, n.m24, n.m31, n.m32, n.m33, n.m34, n.m41, n.m42, n.m43, n.m44]))
}

pub fn pointer(r: Ref, name: StringToken) -> Option<usize> {
    on(r, |a| match a.get(Kind::Ptr, name) {
        Some(Value::Ptr(p)) if p != 0 => Some(p),
        _ => None,
    })
}

pub fn set_constant_buffer(r: Ref, name: StringToken, bytes: &[u8]) {
    on(r, |a| a.set(Kind::ConstantBuffer, name, Value::ConstantBuffer(bytes.into())));
}

/// The bytes managed stored for constant block `name` (with parents).
pub fn constant_buffer(r: Ref, name: &str) -> Option<Arc<[u8]>> {
    on(r, |a| match a.get(Kind::ConstantBuffer, StringToken::of(name)) {
        Some(Value::ConstantBuffer(b)) => Some(b),
        _ => None,
    })
}

/// The combo value `SetComboValue` stored as `name` (with parents).
pub fn combo(r: Ref, name: &str) -> Option<i64> {
    on(r, |a| match a.get(Kind::Combo, StringToken::of(name)) {
        Some(Value::Combo(c)) => Some(i64::from(c)),
        _ => None,
    })
}

/// A numeric value named `name` (with parents) as a shader reads it: float, vector, int, bool or
/// combo, widened to four components.
pub fn shader_value(r: Ref, name: &str) -> Option<glam::Vec4> {
    token_value(r, StringToken::of(name))
}

/// [`shader_value`] by token.
pub fn token_value(r: Ref, t: StringToken) -> Option<glam::Vec4> {
    on(r, |a| {
        [Kind::Vector4, Kind::Vector3, Kind::Vector2, Kind::Float, Kind::Int, Kind::Bool, Kind::Combo, Kind::IntVector4].iter().find_map(|k| match a.get(*k, t)? {
            Value::Vector4(v) => Some(glam::Vec4::new(v.vec.x, v.vec.y, v.vec.z, v.vec.w)),
            Value::Vector3(v) => Some(glam::Vec4::new(v.vec.x, v.vec.y, v.vec.z, 0.0)),
            Value::Vector2(v) => Some(glam::Vec4::new(v.vec.x, v.vec.y, 0.0, 0.0)),
            Value::Float(f) => Some(glam::Vec4::new(f, 0.0, 0.0, 0.0)),
            Value::Int(i) => Some(glam::Vec4::new(i as f32, 0.0, 0.0, 0.0)),
            Value::Bool(b) => Some(glam::Vec4::new(f32::from(u8::from(b)), 0.0, 0.0, 0.0)),
            Value::Combo(c) => Some(glam::Vec4::new(f32::from(c), 0.0, 0.0, 0.0)),
            Value::IntVector4(v) => Some(glam::Vec4::from_array(v.map(|i| i as f32))),
            _ => None,
        })
    })
}

/// Set `name` as native code would: an int, bool, combo, float, buffer, matrix, texture or int
/// vector value.
pub fn set(r: Ref, name: &str, value: Value) {
    let kind = match value {
        Value::Int(_) => Kind::Int,
        Value::Bool(_) => Kind::Bool,
        Value::Combo(_) => Kind::Combo,
        Value::Float(_) => Kind::Float,
        Value::Buffer(_) => Kind::Buffer,
        Value::Matrix(_) => Kind::Matrix,
        Value::Texture(..) => Kind::Texture,
        Value::IntVector4(_) => Kind::IntVector4,
        _ => return,
    };
    on(r, |a| a.set(kind, StringToken::of(name), value));
}

pub fn set_buffer(r: Ref, name: StringToken, handle: usize) {
    on(r, |a| a.set(Kind::Buffer, name, Value::Buffer(handle)));
}

pub fn set_parent(r: Ref, parent: Ref) {
    let parent = objects::get::<_, RenderAttributes>(parent);
    on(r, |s| *s.parent.lock().unwrap() = parent);
}

/// The GPU buffer handle named `name` (with parents), as `SetBufferValue` stored it.
pub fn buffer(r: Ref, name: &str) -> Option<usize> {
    on(r, |a| match a.get(Kind::Buffer, StringToken::of(name)) {
        Some(Value::Buffer(b)) if b != 0 => Some(b),
        _ => None,
    })
}

/// The texture named `name` (with parents).
pub fn texture(r: Ref, name: &str) -> Option<NativeRef<class::CTextureBase>> {
    texture_mip(r, name).map(|(t, _)| t)
}

/// [`texture`] with the mip managed set it for (0 when it asked for none).
pub fn texture_mip(r: Ref, name: &str) -> Option<(NativeRef<class::CTextureBase>, u32)> {
    token_texture(r, StringToken::of(name))
}

/// The sampler state named `name` (with parents).
pub fn sampler(r: Ref, name: &str) -> Option<CSamplerStateDesc> {
    on(r, |a| match a.get(Kind::Sampler, StringToken::of(name)) {
        Some(Value::Sampler(s)) => Some(s),
        _ => None,
    })
}

/// The mip managed named for texture `name`, if it named one (-1, the default, means every mip).
pub fn texture_slice(r: Ref, name: &str) -> Option<u32> {
    on(r, |a| match a.get(Kind::Texture, StringToken::of(name)) {
        Some(Value::Texture(t, mip)) if !t.is_null() && mip >= 0 => Some(mip.unsigned_abs()),
        _ => None,
    })
}

/// [`texture_mip`] by token.
pub fn token_texture(r: Ref, name: StringToken) -> Option<(NativeRef<class::CTextureBase>, u32)> {
    on(r, |a| match a.get(Kind::Texture, name) {
        Some(Value::Texture(t, mip)) if !t.is_null() => Some((t, mip.max(0).unsigned_abs())),
        _ => None,
    })
}

/// Bind a set/get-or-default/delete trio for one value type.
macro_rules! trio {
    ($r:expr, $set:ident, $get:ident, $del:ident, $kind:ident, $ty:ty) => {
        bind!($r, ra::$set, |a, n, v: $ty| on(a, |s| s.set(Kind::$kind, n, Value::$kind(v))));
        bind!($r, ra::$get, |a, n, d: $ty| on(a, |s| match s.get(Kind::$kind, n) {
            Some(Value::$kind(v)) => v,
            _ => d,
        }));
        bind!($r, ra::$del, |a, n| on(a, |s| s.delete(Kind::$kind, n)));
    };
}

pub fn register(r: &mut Registry) {
    bind!(r, ra::CREATE, create);
    bind!(r, ra::DELETE_THIS, |a| {
        objects::destroy(a);
    });

    trio!(r, SET_FLOAT_VALUE, GET_FLOAT_VALUE, DELETE_FLOAT_VALUE, Float, f32);
    trio!(r, SET_VECTOR2_D_VALUE, GET_VECTOR2_D_VALUE, DELETE_VECTOR2_D_VALUE, Vector2, Vector2);
    trio!(r, SET_VECTOR_VALUE, GET_VECTOR_VALUE, DELETE_VECTOR_VALUE, Vector3, Vector3);
    trio!(r, SET_VECTOR4_D_VALUE, GET_VECTOR4_D_VALUE, DELETE_VECTOR4_D_VALUE, Vector4, Vector4);
    trio!(r, SET_V_MATRIX_VALUE, GET_V_MATRIX_VALUE, DELETE_V_MATRIX_VALUE, Matrix, Matrix);
    trio!(r, SET_INT_VALUE, GET_INT_VALUE, DELETE_INT_VALUE, Int, i32);
    trio!(r, SET_COMBO_VALUE, GET_COMBO_VALUE, DELETE_COMBO_VALUE, Combo, u8);
    trio!(r, SET_BOOL_VALUE, GET_BOOL_VALUE, DELETE_BOOL_VALUE, Bool, bool);

    bind!(r, ra::SET_STRING_VALUE, |a, n, v| {
        let v = v.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        on(a, |s| s.set(Kind::String, n, Value::String(v)))
    });
    bind!(r, ra::DELETE_STRING_VALUE, |a, n| on(a, |s| s.delete(Kind::String, n)));
    bind!(r, ra::SET_TEXTURE_VALUE, |a, n, t, mip| on(a, |s| s.set(Kind::Texture, n, Value::Texture(t, mip))));
    bind!(r, ra::GET_TEXTURE_VALUE, |a, n, d| on(a, |s| match s.get(Kind::Texture, n) {
        Some(Value::Texture(t, _)) => t,
        _ => d,
    }));
    bind!(r, ra::DELETE_TEXTURE_VALUE, |a, n| on(a, |s| s.delete(Kind::Texture, n)));
    bind!(r, ra::SET_SAMPLER_VALUE, |a, n, v| on(a, |s| s.set(Kind::Sampler, n, Value::Sampler(v))));
    bind!(r, ra::SET_BUFFER_VALUE, |a, n, b| set_buffer(a, n, b.value));
    bind!(r, ra::SET_PTR_VALUE, |a, n, p| on(a, |s| s.set(Kind::Ptr, n, Value::Ptr(p.as_ptr() as usize))));
    bind!(r, ra::DELETE_PTR_VALUE, |a, n| on(a, |s| s.delete(Kind::Ptr, n)));
    bind!(r, ra::SET_INT_VECTOR4_D_VALUE, |a, n, x, y, z, w| {
        on(a, |s| s.set(Kind::IntVector4, n, Value::IntVector4([x, y, z, w])))
    });
    // Bindless descriptors are the GPU's; the empty device has none to bind.
    bind!(r, ra::SET_GLOBAL_BINDLESS_DESCRIPTOR_SET, |_| {});

    bind!(r, ra::IS_EMPTY, |a| on(a, |s| s.values.lock().unwrap().is_empty()));
    bind!(r, ra::CLEAR, |a, _free_memory, _clear_parent| on(a, |s| s.values.lock().unwrap().clear()));
    bind!(r, ra::SET_PARENT, set_parent);
    bind!(r, ra::MERGE_TO_PTR, |a, into| {
        let from = on(a, |s| s.values.lock().unwrap().clone());
        on(into, |s| s.values.lock().unwrap().extend(from))
    });
    bind!(r, ra::ARE_EQUAL, |a, b, _ignore_combos| a == b);
}
