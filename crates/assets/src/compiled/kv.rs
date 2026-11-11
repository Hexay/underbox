//! Reading compiled KV3 fields leniently (a missing or mistyped field reads as its default, the
//! way the engine's schema loader zero-fills) and building values for the writers.

use kv::{Object, Value};

static EMPTY: Object = Object(Vec::new());

pub fn object<'a>(o: &'a Object, key: &str) -> &'a Object {
    o.get(key).and_then(Value::as_object).unwrap_or(&EMPTY)
}

pub fn array<'a>(o: &'a Object, key: &str) -> &'a [Value] {
    o.get(key).and_then(Value::as_array).unwrap_or_default()
}

/// The objects in an array field, skipping anything else.
pub fn objects<'a>(o: &'a Object, key: &str) -> impl Iterator<Item = &'a Object> {
    array(o, key).iter().filter_map(Value::as_object)
}

pub fn string(o: &Object, key: &str) -> String {
    o.str(key).unwrap_or_default().to_owned()
}

pub fn strings(o: &Object, key: &str) -> Vec<String> {
    array(o, key).iter().filter_map(Value::as_str).map(str::to_owned).collect()
}

pub fn int_of(v: &Value) -> Option<i64> {
    match *v {
        Value::Int(i) => Some(i),
        Value::Bool(b) => Some(i64::from(b)),
        Value::Double(d) => Some(d as i64),
        _ => None,
    }
}

pub fn int(o: &Object, key: &str) -> i64 {
    int_or(o, key, 0)
}

pub fn int_or(o: &Object, key: &str, default: i64) -> i64 {
    o.get(key).and_then(int_of).unwrap_or(default)
}

pub fn ints(o: &Object, key: &str) -> Vec<i64> {
    array(o, key).iter().filter_map(int_of).collect()
}

pub fn float(o: &Object, key: &str) -> f32 {
    float_or(o, key, 0.0)
}

pub fn float_or(o: &Object, key: &str, default: f32) -> f32 {
    o.get(key).and_then(Value::as_f64).map_or(default, |v| v as f32)
}

pub fn bool(o: &Object, key: &str) -> bool {
    o.get(key).and_then(int_of).is_some_and(|v| v != 0)
}

/// A float array value (vectors, quaternions); missing components are 0.
pub fn floats_of<const N: usize>(v: Option<&Value>) -> [f32; N] {
    let items = v.and_then(Value::as_array).unwrap_or_default();
    std::array::from_fn(|i| items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)
}

pub fn vec3(o: &Object, key: &str) -> [f32; 3] {
    floats_of(o.get(key))
}

pub fn vec4(o: &Object, key: &str) -> [f32; 4] {
    floats_of(o.get(key))
}

/// A binary blob field (an empty slice if absent).
pub fn blob<'a>(o: &'a Object, key: &str) -> &'a [u8] {
    match o.get(key) {
        Some(Value::Binary(b)) => b,
        _ => &[],
    }
}

/// KV3 text (a model's key values) as a tree; `None` when blank or unparsable.
pub fn kv3_text(s: &str) -> Option<Value> {
    (!s.trim().is_empty()).then(|| kv::kv3::parse(s).ok().map(|(_, v)| v)).flatten()
}

pub fn obj(members: Vec<(&str, Value)>) -> Value {
    Value::Object(Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

pub fn floats(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&f| Value::Double(f.into())).collect())
}

pub fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}

/// A string flagged as naming a resource, as compiled references are.
pub fn resource(s: &str) -> Value {
    Value::Flagged("resource".into(), Box::new(text(s)))
}

pub fn int_value(i: impl Into<i64>) -> Value {
    Value::Int(i.into())
}

pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(items.into_iter().collect())
}
