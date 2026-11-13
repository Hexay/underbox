//! One `CRenderAttributes` object's values, a namespace per kind, and its parent chain.

use abi::structs::{CSamplerStateDesc, Matrix, Vector2, Vector3, Vector4};
use abi::{NativeRef, StringToken, class};
use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Kind {
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

impl Kind {
    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// The kinds a shader reads as a number, in the order `token_value` prefers them.
pub(super) const NUMERIC: [Kind; 8] = [Kind::Vector4, Kind::Vector3, Kind::Vector2, Kind::Float, Kind::Int, Kind::Bool, Kind::Combo, Kind::IntVector4];

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
struct Values {
    by_kind: HashMap<(Kind, u32), Value>,
    /// Per token, the kinds set: a lookup that misses costs one probe per level, not one per kind.
    kinds: HashMap<u32, u16>,
}

impl Values {
    fn insert(&mut self, kind: Kind, token: u32, value: Value) {
        self.by_kind.insert((kind, token), value);
        *self.kinds.entry(token).or_default() |= kind.bit();
    }

    fn remove(&mut self, kind: Kind, token: u32) {
        self.by_kind.remove(&(kind, token));
        if let Some(k) = self.kinds.get_mut(&token) {
            *k &= !kind.bit();
            if *k == 0 {
                self.kinds.remove(&token);
            }
        }
    }
}

#[derive(Default)]
pub struct RenderAttributes {
    values: Mutex<Values>,
    parent: Mutex<Option<Arc<RenderAttributes>>>,
}

impl RenderAttributes {
    pub(super) fn set(&self, kind: Kind, name: StringToken, value: Value) {
        self.values.lock().unwrap().insert(kind, name.0, value);
    }

    pub(super) fn get(&self, kind: Kind, name: StringToken) -> Option<Value> {
        self.first(&[kind], name).map(|(_, v)| v)
    }

    /// The first of `kinds` set as `name` here or in a parent: each kind is searched up the whole
    /// chain before the next one.
    pub(super) fn first(&self, kinds: &[Kind], name: StringToken) -> Option<(Kind, Value)> {
        let want = kinds.iter().fold(0, |m, k| m | k.bit());
        let mut best: Option<(usize, Value)> = None;
        self.walk(&mut |values| {
            let set = values.kinds.get(&name.0).copied().unwrap_or(0) & want;
            if set == 0 {
                return false;
            }
            let limit = best.as_ref().map_or(kinds.len(), |(i, _)| *i);
            if let Some(i) = kinds[..limit].iter().position(|k| set & k.bit() != 0) {
                best = values.by_kind.get(&(kinds[i], name.0)).map(|v| (i, v.clone()));
            }
            best.as_ref().is_some_and(|(i, _)| *i == 0)
        });
        best.map(|(i, v)| (kinds[i], v))
    }

    /// Visit this level's values, then each parent's, until `f` returns true.
    fn walk(&self, f: &mut dyn FnMut(&Values) -> bool) {
        if f(&self.values.lock().unwrap()) {
            return;
        }
        if let Some(p) = &*self.parent.lock().unwrap() {
            p.walk(f);
        }
    }

    pub(super) fn delete(&self, kind: Kind, name: StringToken) {
        self.values.lock().unwrap().remove(kind, name.0);
    }

    pub(super) fn set_parent(&self, parent: Option<Arc<RenderAttributes>>) {
        *self.parent.lock().unwrap() = parent;
    }

    pub(super) fn is_empty(&self) -> bool {
        self.values.lock().unwrap().by_kind.is_empty()
    }

    pub(super) fn clear(&self) {
        *self.values.lock().unwrap() = Values::default();
    }

    /// Copy this object's own values (not its parents') into `into`.
    pub(super) fn merge_into(&self, into: &RenderAttributes) {
        let from = self.values.lock().unwrap().by_kind.clone();
        let mut to = into.values.lock().unwrap();
        for ((kind, token), value) in from {
            to.insert(kind, token, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(v: Option<(Kind, Value)>) -> Option<(Kind, f32)> {
        v.map(|(k, v)| (k, if let Value::Float(f) = v { f } else { -1.0 }))
    }

    #[test]
    fn kinds_in_order_then_nearest_level() {
        let (t, other) = (StringToken::of("x"), StringToken::of("y"));
        let parent = Arc::new(RenderAttributes::default());
        let child = RenderAttributes::default();
        child.set_parent(Some(parent.clone()));
        parent.set(Kind::Float, t, Value::Float(1.0));
        assert_eq!(float(child.first(&NUMERIC, t)), Some((Kind::Float, 1.0)));
        child.set(Kind::Float, t, Value::Float(2.0));
        assert_eq!(float(child.first(&NUMERIC, t)), Some((Kind::Float, 2.0)));
        child.set(Kind::Int, t, Value::Int(3));
        parent.set(Kind::Vector3, t, Value::Vector3(Vector3::default()));
        assert_eq!(child.first(&NUMERIC, t).map(|(k, _)| k), Some(Kind::Vector3));
        parent.delete(Kind::Vector3, t);
        assert_eq!(float(child.first(&NUMERIC, t)), Some((Kind::Float, 2.0)));
        child.delete(Kind::Float, t);
        assert_eq!(float(child.first(&NUMERIC, t)), Some((Kind::Float, 1.0)));
        assert!(child.first(&NUMERIC, other).is_none() && child.get(Kind::Texture, t).is_none());
    }
}
