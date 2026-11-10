//! HLSL → SPIR-V through Microsoft's DXC, loaded at runtime (hassle-rs). `tools/fetch-dxc.sh`
//! installs the pinned release; without it, [`Dxc::load`] fails and callers skip.

use std::path::{Path, PathBuf};

/// The release `tools/fetch-dxc.sh` pins.
pub const DXC_TAG: &str = "v1.9.2609";

/// Flags for every combo: SM 6.0 Vulkan like native (`VfxCompileTarget_t.SM_6_0_VULKAN`); HLSL
/// 2018 because the sources use per-component vector `?:`, which 2021 rejects; and
/// `-fspv-reflect` for input semantics (stripped again in `spirv::strip_reflection`). Buffers use
/// strict std140/std430 (`-fvk-use-gl-layout`), which is WGSL's layout: DXC's default "relaxed"
/// Vulkan layout and `-fvk-use-dx-layout` both put vectors at offsets WGSL rejects.
pub const ARGS: &[&str] = &["-spirv", "-HV", "2018", "-fspv-target-env=vulkan1.1", "-fvk-use-gl-layout", "-fspv-reflect"];

pub struct Dxc {
    lib: hassle_rs::Dxc,
    pub path: PathBuf,
    /// Hash of the library file, for cache keys.
    pub version: blake3::Hash,
}

impl Dxc {
    /// Loads libdxcompiler: `$UNDERBOX_DXC_LIB` (file or directory), then the fetch script's
    /// install under `$UNDERBOX_DXC_DIR` (default /srv/agents/cache/dxc), then the system search path.
    pub fn load() -> Result<Dxc, String> {
        let file = libloading_name();
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = std::env::var_os("UNDERBOX_DXC_LIB").map(PathBuf::from) {
            candidates.push(if p.is_dir() { p.join(&file) } else { p });
        }
        let root = std::env::var_os("UNDERBOX_DXC_DIR").map_or_else(|| PathBuf::from("/srv/agents/cache/dxc"), PathBuf::from);
        candidates.push(root.join(DXC_TAG).join(if cfg!(windows) { "bin/x64" } else { "lib" }).join(&file));
        let found = candidates.into_iter().find(|p| p.is_file());
        let path = found.clone().unwrap_or_else(|| PathBuf::from(&file));
        let lib = hassle_rs::Dxc::new(Some(path.clone())).map_err(|e| format!("loading {}: {e}", path.display()))?;
        let version = match &found {
            Some(p) => blake3::hash(&std::fs::read(p).map_err(|e| e.to_string())?),
            None => blake3::hash(file.as_bytes()),
        };
        Ok(Dxc { lib, path, version })
    }

    /// Compiles `source` (named `name` in diagnostics) to SPIR-V words.
    pub fn compile(&self, source: &str, name: &Path, entry: &str, profile: &str, defines: &[(String, String)]) -> Result<Vec<u32>, String> {
        let e = |e: hassle_rs::HassleError| e.to_string();
        let compiler = self.lib.create_compiler().map_err(e)?;
        let library = self.lib.create_library().map_err(e)?;
        let blob = library.create_blob_with_encoding_from_str(source).map_err(e)?;
        let defs: Vec<(&str, Option<&str>)> = defines.iter().map(|(k, v)| (k.as_str(), Some(v.as_str()))).collect();
        let name = name.to_string_lossy();
        match compiler.compile(&blob, &name, entry, profile, ARGS, None, &defs) {
            Ok(result) => {
                let bytes = result.get_result().map_err(e)?.to_vec::<u8>();
                if bytes.len() % 4 != 0 {
                    return Err("DXC returned a truncated SPIR-V module".into());
                }
                Ok(bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect())
            }
            Err((result, hr)) => {
                let msg = result
                    .get_error_buffer()
                    .ok()
                    .and_then(|b| library.get_blob_as_string(&b.into()).ok())
                    .unwrap_or_else(|| format!("DXC failed with HRESULT {hr:?}"));
                Err(errors_only(&msg))
            }
        }
    }
}

fn libloading_name() -> String {
    if cfg!(windows) {
        "dxcompiler.dll".into()
    } else if cfg!(target_os = "macos") {
        "libdxcompiler.dylib".into()
    } else {
        "libdxcompiler.so".into()
    }
}

/// DXC's diagnostics without warnings and their context lines.
fn errors_only(msg: &str) -> String {
    let mut out = Vec::new();
    let mut keep = false;
    for line in msg.lines() {
        if line.contains(": error:") || line.contains(": fatal error:") {
            keep = true;
        } else if line.contains(": warning:") || line.contains(": note:") {
            keep = false;
        }
        if keep {
            out.push(line);
        }
    }
    if out.is_empty() { msg.trim().to_string() } else { out.join("\n") }
}
