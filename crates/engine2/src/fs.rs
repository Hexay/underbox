//! The native filesystem (`g_pFullFileSystem`): where native resource loads look. Managed keeps
//! its own filesystem for game resources; this one sees the game projects, cloud folders and
//! symlinks managed registers, plus the engine's core content.

use abi::slots::g_p_full_file_system as fs;
use abi::{Registry, StrRet, bind};
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

#[derive(Default)]
struct Mounts {
    /// Project ident → its Assets folder, in mount order.
    projects: Vec<(String, PathBuf)>,
    /// Cloud folder ident (`mod_cloud`, `mod_transient`) → path.
    cloud: Vec<(String, PathBuf)>,
    /// (path ID, relative path) → real path, consulted first.
    links: HashMap<(String, String), String>,
}

static MOUNTS: LazyLock<RwLock<Mounts>> = LazyLock::new(RwLock::default);

/// Relative resource paths compare case- and separator-blind, as Source's do.
fn key(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The roots searched for a relative path, highest priority first: projects, cloud, core.
pub fn search_roots() -> Vec<PathBuf> {
    let m = MOUNTS.read().unwrap();
    let core = Path::new(&crate::app::platform::game_root()).join("core");
    m.projects.iter().rev().chain(m.cloud.iter()).map(|(_, p)| p.clone()).chain([core]).collect()
}

/// The file a relative resource path names, if any root has it.
pub fn resolve(path: &str, path_id: &str) -> Option<PathBuf> {
    if let Some(real) = MOUNTS.read().unwrap().links.get(&(path_id.to_ascii_uppercase(), key(path))) {
        return Some(PathBuf::from(real));
    }
    let rel = key(path);
    search_roots().into_iter().map(|root| root.join(&rel)).find(|p| p.is_file())
}

fn add_project_path(ident: Option<&CStr>, assets: Option<&CStr>) {
    let assets = PathBuf::from(string(assets));
    // TODO: compile the project's stale or missing _c outputs here (assets crate; decision: compile on mount).
    MOUNTS.write().unwrap().projects.push((string(ident), assets));
}

pub fn register(r: &mut Registry) {
    bind!(r, fs::ADD_PROJECT_PATH, add_project_path);
    bind!(r, fs::ADD_CLOUD_PATH, |ident, path| {
        MOUNTS.write().unwrap().cloud.push((string(ident), PathBuf::from(string(path))));
    });
    bind!(r, fs::RESET_PROJECT_PATHS, |include_cloud| {
        let mut m = MOUNTS.write().unwrap();
        m.projects.clear();
        if include_cloud {
            m.cloud.clear();
        }
    });
    bind!(r, fs::ADD_SYM_LINK, |path, id, real| {
        MOUNTS.write().unwrap().links.insert((string(id).to_ascii_uppercase(), key(&string(path))), string(real));
    });
    bind!(r, fs::REMOVE_SYM_LINK, |path, id| {
        MOUNTS.write().unwrap().links.remove(&(string(id).to_ascii_uppercase(), key(&string(path))));
    });
    bind!(r, fs::GET_SYM_LINK, |path, id| {
        let m = MOUNTS.read().unwrap();
        StrRet::from(m.links.get(&(string(id).to_ascii_uppercase(), key(&string(path)))).cloned())
    });
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn keys_are_case_and_separator_blind() {
        assert_eq!(key("\\Scenes\\Minimal.scene_c"), "scenes/minimal.scene_c");
    }
}
