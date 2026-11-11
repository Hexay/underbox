//! Reading compiled resource files for managed (`EngineGlue.ReadCompiledResourceFile*`), and the
//! compiled texture and material loaders the render path can call ([`texture`], [`material`]),
//! and compiled models' animation data ([`anim`]).

pub mod anim;
#[cfg(test)]
mod anim_tests;
pub mod material;
#[cfg(test)]
mod tests;
pub mod texture;

use abi::slots::engine_glue as glue;
use abi::{RawPtr, Registry, StrRet, bind};
use assets::{container, game_resource};
use std::ffi::{CStr, c_void};

fn json(file: &[u8]) -> StrRet {
    match game_resource::read_json(file) {
        Ok(json) => StrRet::from(json),
        Err(e) => {
            abi::rt::log::warn(&format!("compiled resource: {e:?}"));
            StrRet::Null
        }
    }
}

/// A pointer to the named block's payload inside `file` (managed's own buffer), and its size.
fn block(name: Option<&CStr>, file: &mut [u8], size: &mut i32) -> RawPtr<c_void> {
    *size = 0;
    let Some(kind) = name.and_then(|n| game_resource::fourcc(&n.to_string_lossy())) else { return RawPtr::NULL };
    let Ok(blocks) = container::blocks(file) else { return RawPtr::NULL };
    match blocks.into_iter().find(|b| b.kind == kind) {
        Some(b) => {
            *size = b.len as i32;
            RawPtr::from_raw(file[b.offset..].as_mut_ptr().cast())
        }
        None => RawPtr::NULL,
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, glue::READ_COMPILED_RESOURCE_FILE_JSON, |file: &mut [u8]| json(file));
    bind!(r, glue::READ_COMPILED_RESOURCE_FILE_BLOCK, block);
    bind!(r, glue::READ_COMPILED_RESOURCE_FILE_JSON_FROM_FILESYSTEM, |name| {
        let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match crate::fs::resolve(&name, "GAME").and_then(|p| std::fs::read(p).ok()) {
            Some(file) => json(&file),
            None => StrRet::Null,
        }
    });
}
