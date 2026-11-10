//! Reading typed fields out of decoded KV3.

use kv::{Object, Value};

/// A number, whatever width it was stored at.
pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Flagged(_, inner) => number(inner),
        other => other.as_f64(),
    }
}

/// An array of numbers (vectors, colors, matrix rows).
pub fn floats(v: &Value) -> Option<Vec<f32>> {
    v.as_array()?.iter().map(|x| number(x).map(|n| n as f32)).collect()
}

pub fn field_f32(o: &Object, key: &str) -> Option<f32> {
    o.get(key).and_then(number).map(|n| n as f32)
}

pub fn field_floats<const N: usize>(o: &Object, key: &str) -> Option<[f32; N]> {
    floats(o.get(key)?)?.get(..N)?.try_into().ok()
}

pub fn field_str(o: &Object, key: &str) -> String {
    o.str(key).unwrap_or_default().to_owned()
}

pub fn array<'a>(o: &'a Object, key: &str) -> &'a [Value] {
    o.get(key).and_then(Value::as_array).unwrap_or_default()
}

fn num_text(n: f64) -> String {
    // f32 precision is what entity values hold; print it without trailing noise.
    format!("{}", n as f32)
}

/// An entity value as `CEntityKeyValues::GetValueString` gives it: numbers in decimal, booleans
/// as 1/0, vectors and colors space-separated.
pub fn text(v: &Value) -> String {
    match v {
        Value::Null | Value::Object(_) | Value::Binary(_) => String::new(),
        Value::Bool(b) => u8::from(*b).to_string(),
        Value::Int(i) => i.to_string(),
        Value::Double(d) => num_text(*d),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(" "),
        Value::Flagged(_, inner) => text(inner),
    }
}
