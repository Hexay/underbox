//! Parse every KV1 `.vmat` and KV3 `.vmdl` in an sbox-public checkout ($SBOX_PUBLIC, default
//! ../sbox-public beside the workspace). Skips when there's no checkout.

use std::path::{Path, PathBuf};

fn checkout() -> Option<PathBuf> {
    let p = std::env::var_os("SBOX_PUBLIC")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sbox-public"));
    p.join("game").is_dir().then_some(p)
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

fn check(ext: &str, parse: impl Fn(&str) -> Result<(), kv::Error>) {
    let Some(root) = checkout() else { return eprintln!("no sbox-public checkout; skipped") };
    let mut all = Vec::new();
    files(&root.join("game"), ext, &mut all);
    let failed: Vec<String> = all
        .iter()
        .filter_map(|p| {
            let text = String::from_utf8_lossy(&std::fs::read(p).ok()?).into_owned();
            parse(&text).err().map(|e| format!("{}: {e}", p.display()))
        })
        .collect();
    assert!(failed.is_empty(), "{}/{} .{ext} failed:\n{}", failed.len(), all.len(), failed[..failed.len().min(10)].join("\n"));
    eprintln!("{} .{ext} parsed", all.len());
}

#[test]
fn every_vmat() {
    check("vmat", |s| kv::kv1::parse(s).map(drop));
}

#[test]
fn every_vmdl() {
    check("vmdl", |s| kv::kv3::parse(s).map(drop));
}
