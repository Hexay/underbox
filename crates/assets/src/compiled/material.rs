//! Compiled materials (`.vmat_c`, `MaterialResourceData_t` in DATA): the shader, its int, float,
//! vector and texture parameters, and render attributes. Dynamic (VfxEval) params are skipped.

use super::kv::{self, floats, int_value, list, obj, resource, string, text};
use super::{Error, format};
use ::kv::{Object, Value};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Material {
    pub name: String,
    /// Shader path (`shaders/complex.vfx`).
    pub shader: String,
    /// Includes the static combo choices (`F_*`).
    pub ints: Vec<(String, i64)>,
    pub floats: Vec<(String, f32)>,
    pub vectors: Vec<(String, [f32; 4])>,
    /// (attribute such as `g_tColor`, texture resource path).
    pub textures: Vec<(String, String)>,
    pub int_attributes: Vec<(String, i64)>,
    pub float_attributes: Vec<(String, f32)>,
    pub vector_attributes: Vec<(String, [f32; 4])>,
    pub string_attributes: Vec<(String, String)>,
}

fn params<T>(o: &Object, key: &str, value: impl Fn(&Object) -> T) -> Vec<(String, T)> {
    kv::objects(o, key).map(|p| (string(p, "m_name"), value(p))).collect()
}

pub fn from_kv3(o: &Object) -> Material {
    let int = |p: &Object| kv::int(p, "m_nValue");
    let float = |p: &Object| kv::float(p, "m_flValue");
    let vector = |p: &Object| kv::vec4(p, "m_value");
    Material {
        name: string(o, "m_materialName"),
        shader: string(o, "m_shaderName"),
        ints: params(o, "m_intParams", int),
        floats: params(o, "m_floatParams", float),
        vectors: params(o, "m_vectorParams", vector),
        textures: params(o, "m_textureParams", |p| string(p, "m_pValue")),
        int_attributes: params(o, "m_intAttributes", int),
        float_attributes: params(o, "m_floatAttributes", float),
        vector_attributes: params(o, "m_vectorAttributes", vector),
        string_attributes: params(o, "m_stringAttributes", |p| string(p, "m_value")),
    }
}

pub fn read(file: &[u8]) -> Result<Material, Error> {
    let data = super::kv3_block(file, b"DATA", "DATA")?;
    Ok(from_kv3(data.as_object().ok_or_else(|| format("material DATA isn't an object"))?))
}

fn write_params<T>(items: &[(String, T)], key: &str, value: impl Fn(&T) -> Value) -> Value {
    list(items.iter().map(|(n, v)| obj(vec![("m_name", text(n)), (key, value(v))])))
}

pub fn write_kv3(m: &Material) -> Value {
    let int = |v: &i64| int_value(*v);
    let float = |v: &f32| Value::Double((*v).into());
    let vector = |v: &[f32; 4]| floats(v);
    obj(vec![
        ("m_materialName", text(&m.name)),
        ("m_shaderName", text(&m.shader)),
        ("m_intParams", write_params(&m.ints, "m_nValue", int)),
        ("m_floatParams", write_params(&m.floats, "m_flValue", float)),
        ("m_vectorParams", write_params(&m.vectors, "m_value", vector)),
        ("m_textureParams", write_params(&m.textures, "m_pValue", |t| resource(t))),
        ("m_dynamicParams", list([])),
        ("m_dynamicTextureParams", list([])),
        ("m_intAttributes", write_params(&m.int_attributes, "m_nValue", int)),
        ("m_floatAttributes", write_params(&m.float_attributes, "m_flValue", float)),
        ("m_vectorAttributes", write_params(&m.vector_attributes, "m_value", vector)),
        ("m_textureAttributes", list([])),
        ("m_stringAttributes", write_params(&m.string_attributes, "m_value", |s| text(s))),
        ("m_renderAttributesUsed", list([])),
    ])
}

/// A `.vmat_c` (its textures in the reference list).
pub fn write(m: &Material) -> Vec<u8> {
    let refs: Vec<&str> = m.textures.iter().map(|(_, t)| t.as_str()).collect();
    super::write(0, &refs, vec![(*b"DATA", super::kv3(&write_kv3(m)))])
}
