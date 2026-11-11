//! `g_pResourceCompilerSystem`: compiling sources to `<file>_c`. Only GameResources (JSON) have a
//! compiler here (`assets::game_resource`), as for mounted projects in `fs::add_project_path`.
//! Engine-native types (models, materials, textures, shaders...) report failure; shaders compile
//! through ShaderCompile instead (shader/), which outside the editor frames the `.shader_c` itself.
//!
//! `RunBuildJob` and `ModelGameData` serve only Sandbox.Tools' ContentCompilerHost, a content
//! compiler worker the editor spawns. The editor tables are out of scope (docs/PLAN.md), so a job
//! fails (the host reports it "failed") and there is no `CGameData` (null).

use crate::utl_buffer;
use abi::slots::g_p_resource_compiler_system as rc;
use abi::{RawPtr, Registry, StrRet, bind};
use assets::game_resource;
use std::ffi::{CStr, c_void};
use std::path::{Path, PathBuf};

/// Container version for GameResources, as `assets::project` writes them.
const GAME_RESOURCE_VERSION: u16 = 1;

/// Asset types engine2's own (closed) compilers produce: no GameResource fallback for these.
const NATIVE_TYPES: &[&str] = &[
    "vmdl", "vmat", "vtex", "vpcf", "vsnd", "vanmgrph", "vmap", "vwrld", "vwnod", "shader", "vsurf", "vpost", "vdata",
    "vmorf", "vanim", "vseq", "vnmskel", "vsndevts", "vrman", "vrr", "vfont", "vxml", "vcss", "vjs", "vpk",
];

const IMAGE_TYPES: &[&str] = &["png", "jpg", "jpeg", "tga", "psd", "tif", "tiff", "exr", "pfm", "bmp", "gif", "webp", "hdr"];

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn extension(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_TYPES.contains(&ext.trim_start_matches('.').to_ascii_lowercase().as_str())
}

/// The asset type an extension belongs to: engine types and images are their own; `None` otherwise.
pub fn asset_type_extension(ext: &str) -> Option<String> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    (NATIVE_TYPES.contains(&ext.as_str()) || IMAGE_TYPES.contains(&ext.as_str())).then_some(ext)
}

/// `source` compiled as a GameResource, if its type has no native compiler and it is JSON.
pub fn compile_bytes(path: &str, source: &[u8]) -> Option<Vec<u8>> {
    if NATIVE_TYPES.contains(&extension(path).as_str()) {
        return None;
    }
    let json = std::str::from_utf8(source).ok()?;
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}')).ok()?;
    let rel = path.replace('\\', "/");
    Some(game_resource::compile(json, rel.trim_start_matches('/'), GAME_RESOURCE_VERSION))
}

/// Where `path`'s source lives on disk: as given when absolute, else under a mounted root.
fn on_disk(path: &str) -> PathBuf {
    if Path::new(path).is_absolute() {
        return crate::fs::on_disk(Path::new(path));
    }
    crate::fs::resolve(path, "GAME").unwrap_or_else(|| crate::fs::search_roots().first().map_or_else(|| PathBuf::from(path), |r| r.join(path)))
}

/// Compile `source` (or the file on disk) to `<path>_c` beside the source.
pub fn generate_file(path: &str, source: Option<&[u8]>) -> bool {
    let src = on_disk(path);
    let bytes = match source {
        Some(s) => s.to_vec(),
        None => match std::fs::read(&src) {
            Ok(b) => b,
            Err(_) => return false,
        },
    };
    let Some(compiled) = compile_bytes(path, &bytes) else { return false };
    std::fs::write(PathBuf::from(format!("{}_c", src.display())), compiled).is_ok()
}

/// # Safety
/// `data` must point at `size` readable bytes for the call (managed pins the span).
unsafe fn bytes<'a>(data: RawPtr<c_void>, size: i32) -> &'a [u8] {
    unsafe { data.cast::<u8>().as_slice(size.max(0) as usize) }
}

pub fn register(r: &mut Registry) {
    // SAFETY (all `bytes` below): managed passes a fixed buffer and its length.
    bind!(r, rc::GENERATE_RESOURCE_FILE, |p, data, size| generate_file(&text(p), Some(unsafe { bytes(data, size) })));
    bind!(r, rc::GENERATE_RESOURCE_FILE_1, |p, s| generate_file(&text(p), Some(text(s).as_bytes())));
    bind!(r, rc::GENERATE_RESOURCE_FILE_FORCED, |p, s| generate_file(&text(p), Some(text(s).as_bytes())));
    bind!(r, rc::GENERATE_RESOURCE_BYTES, |p, data, size| {
        compile_bytes(&text(p), unsafe { bytes(data, size) }).map_or(utl_buffer::Ref::NULL, utl_buffer::create)
    });
    bind!(r, rc::COMPILE_FILE, |p| generate_file(&text(p), None));
    bind!(r, rc::FIND_ASSET_TYPE_EXTENSION, |e| StrRet::from(asset_type_extension(&text(e))));
    bind!(r, rc::IS_IMAGE_EXTENSION, |e| is_image_extension(&text(e)));
    // Build jobs never run here, so there's no staging to mount, nothing to unload or pump.
    bind!(r, rc::MOUNT_BUILD_JOB_STAGING, |_, _| {});
    bind!(r, rc::UNLOAD_BUILD_JOBS, || {});
    bind!(r, rc::RUN_MAIN_THREAD_WORK, || {});
    bind!(r, rc::HAS_LOADED_COMPILED_FILE, |_| false);
    bind!(r, rc::RUN_BUILD_JOB, |_mode, _source, _staging, _report, _args| false);
    bind!(r, rc::MODEL_GAME_DATA, || RawPtr::NULL);
    // -1: "can't be calculated here" (ContentCompilerHost.Fingerprint).
    bind!(r, rc::SPECIAL_DEPENDENCY_FINGERPRINT, |_, _, _| -1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_resources_compile_native_types_dont() {
        let out = compile_bytes("scenes\\a.scene", b"{\"GameObjects\":[]}").unwrap();
        assert_eq!(game_resource::read_json(&out), Ok(Some("{\"GameObjects\":[]}".into())));
        assert!(compile_bytes("custom/thing.mytype", b"{}").is_some(), "game-defined types are GameResources");
        assert!(compile_bytes("models/a.vmdl", b"{}").is_none());
        assert!(compile_bytes("a.scene", b"not json").is_none());
    }

    #[test]
    fn writes_beside_an_absolute_source() {
        let dir = std::env::temp_dir().join(format!("engine2-rc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.prefab");
        std::fs::write(&src, "{\"RootObject\":{}}").unwrap();
        assert!(generate_file(src.to_str().unwrap(), None));
        let compiled = std::fs::read(dir.join("a.prefab_c")).unwrap();
        assert_eq!(game_resource::read_json(&compiled), Ok(Some("{\"RootObject\":{}}".into())));
        assert!(generate_file(src.to_str().unwrap(), Some(b"{\"x\":1}")), "override data wins over the file");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extensions() {
        assert!(is_image_extension("PNG") && is_image_extension(".tga") && !is_image_extension("vmdl"));
        assert_eq!(asset_type_extension("vmdl").as_deref(), Some("vmdl"));
        assert_eq!(asset_type_extension("jpg").as_deref(), Some("jpg"));
        assert_eq!(asset_type_extension("scene"), None);
    }
}
