//! Locating sbox-public's core shaders: $SBOX_PUBLIC, else an `sbox-public` beside any ancestor of
//! this crate (works from worktrees too). Tests skip when there's no checkout.

#![allow(dead_code)]

use shaders::IncludePaths;
use std::path::{Path, PathBuf};

pub fn checkout() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SBOX_PUBLIC") {
        return Some(PathBuf::from(p)).filter(|p| p.join("game").is_dir());
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|a| a.join("sbox-public")).find(|p| p.join("game/core/shaders").is_dir())
}

pub fn core_dir() -> Option<PathBuf> {
    checkout().map(|p| p.join("game/core"))
}

pub fn includes(core: &Path) -> IncludePaths {
    IncludePaths::new([core.to_path_buf()])
}

/// Every `.shader` under `dir`, sorted, as (path relative to `dir`, absolute path).
pub fn shader_files(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "shader") {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    walk(dir, &mut all);
    all.sort();
    all.into_iter().map(|p| (p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), p)).collect()
}
