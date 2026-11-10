//! Valve KeyValues: one value model, with parsers for the text forms Source 2 content is
//! authored in (KeyValues1 `.vmat`, KeyValues3 `.vmdl`/`.vpcf`...).

mod cursor;
pub mod json;
pub mod kv1;
pub mod kv3;
pub mod write;

/// A KeyValues value. KV1 produces only `String` and `Object`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<Value>),
    Object(Object),
    /// A KV3 flagged value such as `resource:"x.vmat"`; the flag says what the string names.
    Flagged(String, Box<Value>),
}

/// Members in file order. Keys may repeat (KV1 allows it), so this isn't a map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Object(pub Vec<(String, Value)>);

impl Object {
    /// The first member named `key`, compared case-blind as Source does.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
}

impl Value {
    /// The string itself, or a flagged value's string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            Value::Flagged(_, v) => v.as_str(),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Int(i) => Some(i as f64),
            Value::Double(d) => Some(d),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for Error {}
