//! Compile a project's content folder: every source with a compiler gets its `<file>_c` beside
//! it, when that output is missing or older than the source.

use crate::game_resource;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Report {
    pub compiled: Vec<PathBuf>,
    pub up_to_date: usize,
    pub failed: Vec<(PathBuf, String)>,
}

/// The container `Version` written for GameResources (Facepunch's sample files carry 1).
const GAME_RESOURCE_VERSION: u16 = 1;

/// Compile everything under `root` that's stale. Only GameResources have a compiler so far.
pub fn compile(root: &Path) -> Report {
    let extensions: Vec<&str> = game_resource::engine_extensions().collect();
    let mut report = Report::default();
    let mut sources = Vec::new();
    walk(root, &mut sources);
    for src in sources {
        let Some(ext) = src.extension().and_then(|e| e.to_str()) else { continue };
        if !extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
            continue;
        }
        let out = PathBuf::from(format!("{}_c", src.display()));
        if is_fresh(&src, &out) {
            report.up_to_date += 1;
            continue;
        }
        match compile_one(root, &src, &out) {
            Ok(()) => report.compiled.push(out),
            Err(e) => report.failed.push((src, e)),
        }
    }
    report
}

fn compile_one(root: &Path, src: &Path, out: &Path) -> Result<(), String> {
    let json = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
    let rel = src.strip_prefix(root).unwrap_or(src).to_string_lossy().replace('\\', "/");
    std::fs::write(out, game_resource::compile(&json, &rel, GAME_RESOURCE_VERSION)).map_err(|e| e.to_string())
}

fn is_fresh(src: &Path, out: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((modified(src), modified(out)), (Some(s), Some(o)) if o >= s)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_stale_scenes_once() {
        let dir = std::env::temp_dir().join(format!("assets-project-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::write(dir.join("scenes/a.scene"), "{\"GameObjects\":[]}").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a resource").unwrap();
        let first = compile(&dir);
        assert_eq!((first.compiled.len(), first.up_to_date, first.failed.len()), (1, 0, 0));
        let bytes = std::fs::read(dir.join("scenes/a.scene_c")).unwrap();
        assert_eq!(game_resource::read_json(&bytes), Ok(Some("{\"GameObjects\":[]}".into())));
        let second = compile(&dir);
        assert_eq!((second.compiled.len(), second.up_to_date), (0, 1));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
