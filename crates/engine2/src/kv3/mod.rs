//! `KeyValues3`: native KV3 trees managed builds (model CTRL blocks) and reads (anim events,
//! model game data), plus `EngineGlue`'s KV3/JSON conversions.

mod glue;
pub mod node;
mod scalar;
#[cfg(test)]
mod tests;

pub use node::{Ref, create, to_value};

use abi::slots::key_values3 as kv3;
use abi::structs::Vector3;
use abi::{Registry, StrRet, bind};
use node::{Data, Node, get, handle, handle_opt};
use std::ffi::CStr;
use std::sync::Arc;

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Run `f` on the node's value, or return the default for a dead or null handle.
fn read<R: Default>(r: Ref, f: impl FnOnce(&Data) -> R) -> R {
    get(r).map(|n| f(&n.lock())).unwrap_or_default()
}

fn write(r: Ref, data: Data) {
    if let Some(n) = get(r) {
        n.set(data);
    }
}

fn member<R>(r: Ref, key: Option<&CStr>, default: R, f: impl FnOnce(&Data) -> Option<R>) -> R {
    get(r).and_then(|n| n.member(&text(key))).and_then(|m| f(&m.lock())).unwrap_or(default)
}

fn set_member(r: Ref, key: Option<&CStr>, data: Data) {
    if let Some(n) = get(r) {
        n.find_or_create(&text(key)).set(data);
    }
}

fn nth_member(r: Ref, i: i32) -> Option<(String, Arc<Node>)> {
    let n = get(r)?;
    match &*n.lock() {
        Data::Table(members) => members.get(usize::try_from(i).ok()?).cloned(),
        _ => None,
    }
}

fn string(s: Option<&CStr>, resource: bool) -> Data {
    Data::String { text: text(s), resource }
}

pub fn register(r: &mut Registry) {
    bind!(r, kv3::CREATE, || handle(&Node::new(Data::Null)));
    bind!(r, kv3::DELETE_THIS, |r| {
        if let Some(n) = get(r) {
            node::release(&n);
        }
    });
    bind!(r, kv3::IS_ARRAY, |r| read(r, |d| matches!(d, Data::Array(_))));
    bind!(r, kv3::IS_TABLE, |r| read(r, |d| matches!(d, Data::Table(_))));
    bind!(r, kv3::GET_TYPE, |r| get(r).map_or(node::TYPE_INVALID, |n| n.lock().type_id()));

    bind!(r, kv3::GET_VALUE_BOOL, |r| read(r, |d| d.as_bool().unwrap_or_default()));
    bind!(r, kv3::GET_VALUE_INT, |r| read(r, |d| d.as_i64().unwrap_or_default() as i32));
    bind!(r, kv3::GET_VALUE_INT64, |r| read(r, |d| d.as_i64().unwrap_or_default()));
    bind!(r, kv3::GET_VALUE_UINT64, |r| read(r, |d| d.as_u64().unwrap_or_default()));
    bind!(r, kv3::GET_VALUE_FLOAT, |r| read(r, |d| d.as_f64().unwrap_or_default() as f32));
    bind!(r, kv3::GET_VALUE_DOUBLE, |r| read(r, |d| d.as_f64().unwrap_or_default()));
    bind!(r, kv3::GET_VALUE_STRING, |r| StrRet::from(read(r, Data::as_string)));
    bind!(r, kv3::GET_VALUE_VECTOR, |r| read(r, |d| d.as_vector().unwrap_or_default()));
    bind!(r, kv3::GET_VALUE_COLOR, |r| read(r, |d| d.as_color().unwrap_or_default()));

    bind!(r, kv3::SET_VALUE_BOOL, |r, v| write(r, Data::Bool(v)));
    bind!(r, kv3::SET_VALUE_INT, |r, v: i32| write(r, Data::Int(v.into())));
    bind!(r, kv3::SET_VALUE_FLOAT, |r, v: f32| write(r, Data::Double(v.into())));
    bind!(r, kv3::SET_VALUE_STRING, |r, v| write(r, string(v, false)));
    bind!(r, kv3::SET_VALUE_RESOURCE_STRING, |r, v| write(r, string(v, true)));

    bind!(r, kv3::SET_MEMBER_STRING, |r, k, v| set_member(r, k, string(v, false)));
    bind!(r, kv3::SET_MEMBER_INT, |r, k, v: i32| set_member(r, k, Data::Int(v.into())));
    bind!(r, kv3::SET_MEMBER_FLOAT, |r, k, v: f32| set_member(r, k, Data::Double(v.into())));
    bind!(r, kv3::GET_MEMBER_STRING, |r, k| StrRet::from(member(r, k, String::new(), |d| Some(d.as_string()))));
    bind!(r, kv3::GET_MEMBER_INT, |r, k, def| member(r, k, def, |d| d.as_i64().map(|i| i as i32)));
    bind!(r, kv3::GET_MEMBER_FLOAT, |r, k, def| member(r, k, def, |d| d.as_f64().map(|f| f as f32)));
    bind!(r, kv3::GET_MEMBER_VECTOR, |r, k, def: Vector3| member(r, k, def, Data::as_vector));

    bind!(r, kv3::SET_TO_EMPTY_ARRAY, |r| write(r, Data::Array(Vec::new())));
    bind!(r, kv3::SET_TO_EMPTY_TABLE, |r| write(r, Data::Table(Vec::new())));
    bind!(r, kv3::GET_ARRAY_LENGTH, |r| read(r, |d| if let Data::Array(a) = d { a.len() as i32 } else { 0 }));
    bind!(r, kv3::ARRAY_ADD_TO_TAIL, |r| handle_opt(get(r).map(|n| n.push())));
    bind!(r, kv3::GET_ARRAY_ELEMENT, |r, i| {
        let element = |d: &Data| match d {
            Data::Array(a) => usize::try_from(i).ok().and_then(|i| a.get(i).cloned()),
            _ => None,
        };
        handle_opt(get(r).and_then(|n| element(&n.lock())))
    });
    bind!(r, kv3::FIND_OR_CREATE_MEMBER, |r, k| handle_opt(get(r).map(|n| n.find_or_create(&text(k)))));
    bind!(r, kv3::GET_MEMBER_COUNT, |r| read(r, |d| if let Data::Table(t) = d { t.len() as i32 } else { 0 }));
    bind!(r, kv3::GET_MEMBER, |r, i| handle_opt(nth_member(r, i).map(|(_, n)| n)));
    bind!(r, kv3::GET_MEMBER_NAME, |r, i| StrRet::from(nth_member(r, i).map(|(k, _)| k)));

    bind!(r, kv3::SAVE, |r| crate::utl_buffer::create(save(r).into_bytes()));
    bind!(r, kv3::SAVE_TO_FILE, |r, path| save_to_file(r, &text(path)));
    glue::register(r);
}

/// The tree as KV3 text, valid for any KV3 loader. TODO: native's binary auto-compressed encoding.
pub fn save(r: Ref) -> String {
    let value = get(r).map_or(kv::Value::Null, |n| to_value(&n));
    kv::write::kv3(&value)
}

fn save_to_file(r: Ref, path: &str) -> bool {
    !path.is_empty() && get(r).is_some() && std::fs::write(path, save(r)).is_ok()
}
