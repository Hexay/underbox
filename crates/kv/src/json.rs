//! KeyValues <-> JSON, as engine2's `EngineGlue` converts them for managed's System.Text.Json.

use crate::{Object, Value};
use serde_json::{Map, Number, Value as Json};

/// Tables become objects, flagged values their plain value, binary blobs arrays of bytes.
pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Double(d) => Number::from_f64(*d).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(s.clone()),
        Value::Binary(b) => Json::Array(b.iter().map(|&b| Json::from(b)).collect()),
        Value::Array(items) => Json::Array(items.iter().map(to_json).collect()),
        Value::Object(o) => Json::Object(o.0.iter().map(|(k, v)| (k.clone(), to_json(v))).collect::<Map<_, _>>()),
        Value::Flagged(_, v) => to_json(v),
    }
}

/// Integers that fit stay integers; u64 beyond `i64::MAX` and fractions become doubles.
pub fn from_json(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n.as_i64().map_or_else(|| Value::Double(n.as_f64().unwrap_or(0.0)), Value::Int),
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::Array(items.iter().map(from_json).collect()),
        Json::Object(m) => Value::Object(Object(m.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_kinds() {
        let j: Json = serde_json::from_str(r#"{"z":1,"a":[true,null,2.5,"s"],"o":{"n":-7}}"#).unwrap();
        let v = from_json(&j);
        let o = v.as_object().unwrap();
        assert_eq!(o.get("z"), Some(&Value::Int(1)));
        assert_eq!(o.get("a").unwrap().as_array().unwrap()[2], Value::Double(2.5));
        assert_eq!(to_json(&v), j);
    }

    #[test]
    fn flagged_and_binary() {
        let v = Value::Array(vec![Value::Flagged("resource".into(), Box::new(Value::String("x.vmat".into()))), Value::Binary(vec![1, 255])]);
        assert_eq!(to_json(&v).to_string(), r#"["x.vmat",[1,255]]"#);
    }
}
