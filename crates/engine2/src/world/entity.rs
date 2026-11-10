//! `CEntityKeyValues`: one map entity's values, keyed by `StringToken` as managed asks for them
//! (`MapLoader.ObjectEntry` reads every key by index, then each value by key).

use abi::slots::c_entity_key_values as ekv;
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use std::ffi::CStr;
use world::entities::{Entity, Key};

pub type Ref = NativeRef<class::CEntityKeyValues>;

pub struct KeyValues {
    values: Vec<(u32, String)>,
}

/// A native copy of `e`, owned by its world reference.
pub fn create(e: &Entity) -> Ref {
    let token = |k: &Key| match k {
        Key::Name(n) => StringToken::of(n).0,
        Key::Hash(h) => *h,
    };
    objects::create(KeyValues { values: e.values.iter().map(|(k, v)| (token(k), v.clone())).collect() })
}

fn on<R: Default>(r: Ref, f: impl FnOnce(&KeyValues) -> R) -> R {
    objects::get::<_, KeyValues>(r).map(|kv| f(&kv)).unwrap_or_default()
}

pub fn lookup(r: Ref, token: u32) -> Option<String> {
    on(r, |kv| kv.values.iter().find(|(t, _)| *t == token).map(|(_, v)| v.clone()))
}

fn value(r: Ref, token: u32, default: Option<&CStr>) -> StrRet {
    StrRet::from(lookup(r, token).or_else(|| default.map(|d| d.to_string_lossy().into_owned())))
}

pub fn register(r: &mut Registry) {
    bind!(r, ekv::GET_VALUE_STRING, |r, key, default| value(r, key.0, default));
    bind!(r, ekv::GET_VALUE_STRING_1, value);
    bind!(r, ekv::GET_KEY_COUNT, |r| on(r, |kv| kv.values.len() as i32));
    bind!(r, ekv::GET_KEY, |r, i| on(r, |kv| usize::try_from(i).ok().and_then(|i| kv.values.get(i)).map_or(0, |(t, _)| *t)));
}
