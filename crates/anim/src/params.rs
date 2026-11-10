//! Graph control parameters: their definitions and one instance's current values.

use crate::xform::Xform;
use glam::{Quat, Vec3};
use std::sync::Arc;

/// A bone or a transform a graph aims at (`SetParamTarget`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Target {
    pub set: bool,
    pub bone: Option<String>,
    pub xform: Xform,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    /// An index into the parameter's options (AnimGraph1 enums).
    Enum(u8),
    /// An interned name (AnimGraph2 IDs); empty is unset.
    Id(String),
    Vector(Vec3),
    Rotation(Quat),
    Target(Target),
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Enum(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Id(s) => !s.is_empty(),
            Value::Target(t) => t.set,
            Value::Vector(_) | Value::Rotation(_) => false,
        }
    }

    pub fn as_float(&self) -> f32 {
        match self {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f32,
            Value::Enum(i) => *i as f32,
            Value::Bool(b) => *b as u8 as f32,
            _ => 0.0,
        }
    }

    fn same_kind(&self, other: &Value) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub default: Value,
    pub min: f32,
    pub max: f32,
    /// Enum option names (AnimGraph1) or expected IDs (AnimGraph2).
    pub options: Vec<String>,
}

impl ParamDef {
    pub fn new(name: &str, default: Value) -> Self {
        Self { name: name.to_owned(), default, min: 0.0, max: 0.0, options: Vec::new() }
    }
}

/// One graph instance's parameter values, reset to the definitions' defaults.
#[derive(Debug, Clone)]
pub struct Params {
    defs: Arc<[ParamDef]>,
    values: Vec<Value>,
}

impl Params {
    pub fn new(defs: Arc<[ParamDef]>) -> Self {
        let values = defs.iter().map(|d| d.default.clone()).collect();
        Self { defs, values }
    }

    pub fn defs(&self) -> &[ParamDef] {
        &self.defs
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.defs.iter().position(|d| d.name.eq_ignore_ascii_case(name))
    }

    pub fn get(&self, i: usize) -> Option<&Value> {
        self.values.get(i)
    }

    pub fn by_name(&self, name: &str) -> Option<&Value> {
        self.index(name).and_then(|i| self.get(i))
    }

    /// Set parameter `i`; false (and unchanged) if it doesn't exist or `value` is another kind.
    /// Numbers convert between Bool/Int/Float/Enum the way managed's AnimVariant does.
    pub fn set(&mut self, i: usize, value: Value) -> bool {
        let Some(slot) = self.values.get_mut(i) else { return false };
        let converted = match (&*slot, value) {
            (cur, v) if cur.same_kind(&v) => v,
            (Value::Float(_), v @ (Value::Int(_) | Value::Bool(_) | Value::Enum(_))) => Value::Float(v.as_float()),
            (Value::Int(_), v @ (Value::Float(_) | Value::Bool(_) | Value::Enum(_))) => Value::Int(v.as_float() as i32),
            (Value::Enum(_), v @ (Value::Float(_) | Value::Int(_))) => Value::Enum(v.as_float().clamp(0.0, 255.0) as u8),
            (Value::Bool(_), v @ (Value::Float(_) | Value::Int(_) | Value::Enum(_))) => Value::Bool(v.as_bool()),
            _ => return false,
        };
        *slot = converted;
        true
    }

    pub fn set_by_name(&mut self, name: &str, value: Value) -> bool {
        self.index(name).is_some_and(|i| self.set(i, value))
    }

    pub fn reset(&mut self, i: usize) -> bool {
        let Some(d) = self.defs.get(i) else { return false };
        self.values[i] = d.default.clone();
        true
    }

    pub fn reset_all(&mut self) {
        for (v, d) in self.values.iter_mut().zip(self.defs.iter()) {
            *v = d.default.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_convert_reset() {
        let defs: Arc<[ParamDef]> = vec![ParamDef::new("move_x", Value::Float(1.0)), ParamDef::new("b_jump", Value::Bool(false))].into();
        let mut p = Params::new(defs);
        assert!(p.set_by_name("MOVE_X", Value::Int(3)));
        assert_eq!(p.by_name("move_x"), Some(&Value::Float(3.0)));
        assert!(!p.set_by_name("move_x", Value::Id("x".into())), "kind mismatch is refused");
        assert!(!p.set_by_name("missing", Value::Bool(true)));
        assert!(p.set_by_name("b_jump", Value::Float(1.0)));
        assert!(p.by_name("b_jump").unwrap().as_bool());
        p.reset_all();
        assert_eq!(p.get(0), Some(&Value::Float(1.0)));
    }
}
