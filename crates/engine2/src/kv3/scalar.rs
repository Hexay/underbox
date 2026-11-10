//! Reading a KV3 value as another type, as `KeyValues3::GetInt`/`GetFloat`/`GetString`... convert:
//! numbers and bools cross-convert, numeric strings parse, containers read as nothing.

use super::node::Data;
use crate::math::vec3;
use abi::structs::{Color32, Vector3};

impl Data {
    pub fn type_id(&self) -> i64 {
        match self {
            Data::Null => 1,
            Data::Bool(_) => 2,
            Data::Int(_) => 3,
            Data::UInt(_) => 4,
            Data::Double(_) => 5,
            Data::String { .. } => 6,
            Data::Binary(_) => 7,
            Data::Array(_) => 8,
            Data::Table(_) => 9,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Data::Bool(b) => Some(f64::from(u8::from(*b))),
            Data::Int(i) => Some(*i as f64),
            Data::UInt(u) => Some(*u as f64),
            Data::Double(d) => Some(*d),
            Data::String { text, .. } => text.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Data::Int(i) => Some(*i),
            Data::UInt(u) => Some(*u as i64),
            Data::String { text, .. } => text.trim().parse().ok().or_else(|| self.as_f64().map(|d| d as i64)),
            _ => self.as_f64().map(|d| d as i64),
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Data::UInt(u) => Some(*u),
            Data::String { text, .. } if let Ok(u) = text.trim().parse() => Some(u),
            _ => self.as_i64().map(|i| i as u64),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Data::Bool(b) => Some(*b),
            Data::String { text, .. } if text.eq_ignore_ascii_case("true") => Some(true),
            Data::String { text, .. } if text.eq_ignore_ascii_case("false") => Some(false),
            _ => self.as_f64().map(|d| d != 0.0),
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Data::String { text, .. } => text.clone(),
            Data::Bool(b) => b.to_string(),
            Data::Int(i) => i.to_string(),
            Data::UInt(u) => u.to_string(),
            Data::Double(d) => d.to_string(),
            _ => String::new(),
        }
    }

    /// The first `N` numbers of an array value.
    fn numbers<const N: usize>(&self) -> Option<[f64; N]> {
        let Data::Array(items) = self else { return None };
        if items.len() < N {
            return None;
        }
        let mut out = [0.0; N];
        for (o, n) in out.iter_mut().zip(items) {
            *o = n.lock().as_f64()?;
        }
        Some(out)
    }

    /// A vector is stored as `[x, y, z]`.
    pub fn as_vector(&self) -> Option<Vector3> {
        self.numbers::<3>().map(|[x, y, z]| vec3(x as f32, y as f32, z as f32))
    }

    /// A color is stored as `[r, g, b]` or `[r, g, b, a]`, 0-255 each.
    pub fn as_color(&self) -> Option<Color32> {
        let byte = |v: f64| v.clamp(0.0, 255.0) as u8;
        let a = self.numbers::<4>().map_or(255, |c| byte(c[3]));
        self.numbers::<3>().map(|[r, g, b]| Color32 { r: byte(r), g: byte(g), b: byte(b), a })
    }
}
