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
    /// `EngineGlue.AddSearchPath` roots (path ID, folder): ahead of everything, or behind the cloud.
    front: Vec<(String, PathBuf)>,
    back: Vec<(String, PathBuf)>,
    /// Mounted map paks, searched after the folders, newest first.
    paks: Vec<std::sync::Arc<world::vpk::Vpk>>,
}

/// Make a pak's files readable through [`read`] until [`unmount_pak`].
pub fn mount_pak(pak: std::sync::Arc<world::vpk::Vpk>) {
    MOUNTS.write().unwrap().paks.push(pak);
}

pub fn unmount_pak(pak: &std::sync::Arc<world::vpk::Vpk>) {
    MOUNTS.write().unwrap().paks.retain(|p| !std::sync::Arc::ptr_eq(p, pak));
}

/// A file's bytes: from the folders [`resolve`] searches, else from a mounted pak.
pub fn read(path: &str, path_id: &str) -> Option<Vec<u8>> {
    if let Some(file) = resolve(path, path_id) {
        return std::fs::read(file).ok();
    }
    let paks = MOUNTS.read().unwrap().paks.clone();
    paks.iter().rev().find_map(|p| p.read(path)?.inspect_err(|e| abi::rt::log::warn(&e.to_string())).ok())
}

/// Add a search root for `path_id`, ahead of the others when `front`.
pub fn add_search_path(path_id: &str, path: &str, front: bool) {
    let mut m = MOUNTS.write().unwrap();
    let list = if front { &mut m.front } else { &mut m.back };
    list.push((path_id.to_ascii_uppercase(), PathBuf::from(path)));
}

/// Drop a root [`add_search_path`] added; false if there was none.
pub fn remove_search_path(path_id: &str, path: &str) -> bool {
    let entry = (path_id.to_ascii_uppercase(), PathBuf::from(path));
    let mut m = MOUNTS.write().unwrap();
    let before = m.front.len() + m.back.len();
    m.front.retain(|e| *e != entry);
    m.back.retain(|e| *e != entry);
    m.front.len() + m.back.len() < before
}

static MOUNTS: LazyLock<RwLock<Mounts>> = LazyLock::new(RwLock::default);

/// Relative resource paths compare case- and separator-blind, as Source's do.
fn key(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The roots searched for a relative path, highest priority first: added-to-front, projects,
/// cloud, added-to-back, core.
pub fn search_roots() -> Vec<PathBuf> {
    let m = MOUNTS.read().unwrap();
    let core = Path::new(&crate::app::platform::game_root()).join("core");
    let ordered = m.front.iter().rev().chain(m.projects.iter().rev()).chain(m.cloud.iter()).chain(m.back.iter());
    ordered.map(|(_, p)| p.clone()).chain([core]).collect()
}

/// The file a relative resource path names, if any root has it.
pub fn resolve(path: &str, path_id: &str) -> Option<PathBuf> {
    if let Some(real) = MOUNTS.read().unwrap().links.get(&(path_id.to_ascii_uppercase(), key(path))) {
        return Some(PathBuf::from(real));
    }
    let rel = key(path);
    search_roots().into_iter().find_map(|root| {
        let exact = root.join(&rel);
        if exact.is_file() {
            return Some(exact);
        }
        // Keys are lowercase; a game authored on Windows may spell folders `Audio/Ui`.
        let (found, complete) = join_case_blind(root, Path::new(&rel));
        (complete && found.is_file()).then_some(found)
    })
}

/// `base/rel` as spelled on disk, matching each component case-blind. Stops at the first component
/// no spelling matches and appends the rest as given; the flag says whether every component matched.
fn join_case_blind(base: PathBuf, rel: &Path) -> (PathBuf, bool) {
    let mut out = base;
    let mut parts = rel.components();
    while let Some(part) = parts.next() {
        let exact = out.join(part);
        if exact.exists() {
            out = exact;
            continue;
        }
        let wanted = part.as_os_str().to_string_lossy().to_lowercase();
        let found = std::fs::read_dir(&out).into_iter().flatten().flatten().map(|e| e.file_name()).find(|n| n.to_string_lossy().to_lowercase() == wanted);
        match found {
            Some(name) => out.push(name),
            None => return (out.join(part).join(parts.as_path()), false),
        }
    }
    (out, true)
}

/// `path` as it's spelled on disk, matching each missing component case-blind. Managed lowercases
/// some paths it hands us (AddProjectPath), which only works on Windows' case-blind filesystems.
pub fn on_disk(path: &Path) -> PathBuf {
    join_case_blind(PathBuf::new(), path).0
}

fn add_project_path(ident: Option<&CStr>, assets: Option<&CStr>) {
    let assets = on_disk(Path::new(&string(assets)));
    // Managed loads the project's resources right after mounting it, so stale outputs compile now.
    let report = assets::project::compile(&assets);
    for (src, e) in &report.failed {
        abi::rt::log::warn(&format!("compile {}: {e}", src.display()));
    }
    abi::rt::log::log(
        abi::rt::log::Level::Info,
        &format!("{}: {} compiled, {} up to date", assets.display(), report.compiled.len(), report.up_to_date),
    );
    MOUNTS.write().unwrap().projects.push((string(ident), assets));
}

pub fn register(r: &mut Registry) {
    bind!(r, fs::ADD_PROJECT_PATH, add_project_path);
    bind!(r, fs::ADD_CLOUD_PATH, |ident, path| {
        MOUNTS.write().unwrap().cloud.push((string(ident), on_disk(Path::new(&string(path)))));
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
    use super::{add_search_path, key, on_disk, remove_search_path, resolve};

    #[test]
    fn keys_are_case_and_separator_blind() {
        assert_eq!(key("\\Scenes\\Minimal.scene_c"), "scenes/minimal.scene_c");
    }

    #[test]
    fn resolve_finds_mixed_case_folders() {
        let root = std::env::temp_dir().join(format!("underbox_fs_case_{}", std::process::id()));
        std::fs::create_dir_all(root.join("Audio/Ui")).unwrap();
        std::fs::write(root.join("Audio/Ui/Cancel.ogg"), b"x").unwrap();
        let root_s = root.to_string_lossy().into_owned();
        add_search_path("UNDERBOX_FS_CASE", &root_s, true);
        let found = resolve("audio/ui/cancel.ogg", "GAME");
        let missing = resolve("audio/ui/absent.ogg", "GAME");
        remove_search_path("UNDERBOX_FS_CASE", &root_s);
        assert_eq!(found, Some(root.join("Audio/Ui/Cancel.ogg")));
        assert_eq!(missing, None);
        assert_eq!(on_disk(&root.join("audio/ui/absent/x")), root.join("Audio/Ui/absent/x"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
