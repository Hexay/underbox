//! `EngineGlue`'s KeyValues conversions: KV3 text and JSON to native trees and back, and legacy
//! KeyValues1 text (old `.cfg`/key binding files) to JSON.

use super::node::{Ref, create, get, to_value};
use super::text;
use abi::slots::engine_glue as glue;
use abi::{Registry, StrRet, bind};

pub fn json_to_kv3(json: &str) -> Ref {
    serde_json::from_str(json).map_or(Ref::NULL, |j| create(&kv::json::from_json(&j)))
}

pub fn kv3_to_json(r: Ref) -> Option<String> {
    get(r).map(|n| kv::json::to_json(&to_value(&n)).to_string())
}

pub fn load_kv3(src: &str) -> Ref {
    kv::kv3::parse(src).map_or(Ref::NULL, |(_, v)| create(&v))
}

/// JSON of the first root block's members, as `KeyValues::LoadFromBuffer` keeps only that block.
pub fn kv1_to_json(src: &str) -> Option<String> {
    let root = kv::kv1::parse(src).ok()?;
    let (_, first) = root.0.first()?;
    Some(kv::json::to_json(first).to_string())
}

pub fn register(r: &mut Registry) {
    bind!(r, glue::JSON_TO_KEY_VALUES3, |s| json_to_kv3(&text(s)));
    bind!(r, glue::KEY_VALUES3_TO_JSON, |r| StrRet::from(kv3_to_json(r)));
    bind!(r, glue::LOAD_KEY_VALUES3, |s| load_kv3(&text(s)));
    bind!(r, glue::KEY_VALUES_TO_JSON, |s| StrRet::from(kv1_to_json(&text(s))));
}
