//! Real games' compiled GameResources and particle snapshots through our readers, when
//! `UNDERBOX_REAL_RESOURCES` lists folders of them (`:`-separated). Skipped otherwise: none are or
//! may be in this repository (docs/real-games.md).

use crate::{container, game_resource, kv3bin};
use std::path::{Path, PathBuf};

const GAME_RESOURCES: &[&str] = &["geq_c", "sdflayer_c", "surface_c"];

fn find_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_files(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| GAME_RESOURCES.contains(&x) || x == "vsnap_c") {
            out.push(p);
        }
    }
}

fn check(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let kinds: Vec<String> = container::blocks(&bytes).map_err(|e| format!("{e:?}"))?.iter().map(|b| String::from_utf8_lossy(&b.kind).into_owned()).collect();
    if file.extension().is_some_and(|x| x == "vsnap_c") {
        let data = container::block(&bytes, b"DATA").map_err(|e| format!("{e:?}"))?.ok_or("no DATA")?;
        kv3bin::read(data).map_err(|e| format!("DATA isn't binary KV3: {e:?}"))?;
        return Ok(format!("blocks {kinds:?}, DATA binary KV3"));
    }
    let json = game_resource::read_json(&bytes).map_err(|e| format!("{e:?}"))?.ok_or("no DATA")?;
    if !json.trim_start().starts_with('{') {
        return Err(format!("DATA isn't JSON: {:?}", json.get(..16)));
    }
    Ok(format!("blocks {kinds:?}, {} bytes of JSON", json.len()))
}

#[test]
fn real_resource_files_read() {
    let Ok(dirs) = std::env::var("UNDERBOX_REAL_RESOURCES") else { return eprintln!("skipped: UNDERBOX_REAL_RESOURCES unset") };
    let mut files = Vec::new();
    dirs.split(':').for_each(|d| find_files(Path::new(d), &mut files));
    files.sort();
    assert!(!files.is_empty(), "no compiled resources under {dirs}");
    let mut failed = 0;
    for f in &files {
        let line = check(f).unwrap_or_else(|e| {
            failed += 1;
            format!("FAILED {e}")
        });
        println!("{}: {line}", f.file_name().unwrap().to_string_lossy());
    }
    assert_eq!(failed, 0, "{failed} of {} files failed", files.len());
}
