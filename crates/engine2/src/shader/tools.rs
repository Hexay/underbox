//! `ShaderTools`: the versions a `.shader_c` must carry (compiled.rs), a shader's source, and
//! native's per-program masking (`VfxMaskUnusedParts`).

use super::compiled::{ABI_VERSION, MIN_VCS_VERSION, RESOURCE_VERSION, VCS_VERSION};
use super::{locate, stage, string};
use abi::slots::shader_tools as st;
use abi::{Registry, StrRet, bind};
use shaders::Stage;

/// The text of `path` (absolute, else under the mounted roots), or "" as native's `ReadVfxFile` fails.
pub(super) fn source(path: &str) -> String {
    locate(path).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default()
}

/// `src` with the stage blocks other than `program`'s blanked (FEATURE keeps none); HEADER,
/// FEATURES, MODES, COMMON and loose code stay. `for_crc` also drops comments. Source the block
/// scanner can't split comes back unchanged, as native returns its buffer either way.
pub(super) fn mask(src: &str, program: i64, for_crc: bool) -> String {
    let own = stage(program).map(Stage::block);
    let is_stage = |name: &str| Stage::ALL.iter().any(|s| s.block() == name) || name == "PS_RENDER_STATE";
    let keep = |name: &str| !is_stage(name) || Some(name) == own || (program == 7 && name == "PS_RENDER_STATE");
    shaders::blocks::mask(src, keep, for_crc).unwrap_or_else(|_| src.to_string())
}

pub fn register(r: &mut Registry) {
    bind!(r, st::GET_RESOURCE_VERSION, || RESOURCE_VERSION);
    bind!(r, st::GET_VCS_VERSION, || VCS_VERSION);
    bind!(r, st::GET_MIN_VCS_VERSION, || MIN_VCS_VERSION);
    bind!(r, st::GET_ABI_VERSION, || ABI_VERSION);
    bind!(r, st::GET_SHADER_SOURCE, |p| StrRet::from(source(&string(p))));
    bind!(r, st::MASK_SHADER_SOURCE, |src, program, for_crc| StrRet::from(mask(&string(src), program, for_crc)));
}
