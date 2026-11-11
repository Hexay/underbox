//! Core GameResources compile at boot and resolve by path, as a real install's compiled core does.

use super::{add_search_path, read, remove_search_path};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("underbox_fs_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Compile `core` twice (the second pass must find everything fresh), mount it, and parse each
/// lookup's JSON. Returns how many sources compiled.
fn compile_and_look_up(core: &Path, lookups: &[&str]) -> usize {
    let first = assets::project::compile(core);
    assert!(first.failed.is_empty(), "{:?}", first.failed);
    let second = assets::project::compile(core);
    assert_eq!((second.compiled.len(), second.up_to_date), (0, first.compiled.len()));
    let id = format!("UNDERBOX_FS_CORE_{}", core.display());
    add_search_path(&id, &core.to_string_lossy(), true);
    let found: Vec<_> = lookups.iter().map(|p| read(&p.to_uppercase(), &id).map(|b| assets::game_resource::read_json(&b))).collect();
    remove_search_path(&id, &core.to_string_lossy());
    for (path, json) in lookups.iter().zip(found) {
        let json = json.unwrap_or_else(|| panic!("{path} not found")).unwrap().unwrap();
        serde_json::from_str::<serde_json::Value>(&json).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    first.compiled.len()
}

#[test]
fn synthetic_core_resources_resolve() {
    let core = scratch("core_synth");
    for (rel, json) in [
        ("surfaces/default.surface", r#"{"Friction":0.8}"#),
        ("sounds/ui/click.sound", r#"{"Volume":1}"#),
        ("prefabs/effects/spark.prefab", r#"{"RootObject":{}}"#),
    ] {
        std::fs::create_dir_all(core.join(rel).parent().unwrap()).unwrap();
        std::fs::write(core.join(rel), json).unwrap();
    }
    std::fs::write(core.join("surfaces/default.surface.meta"), "{}").unwrap();
    let n = compile_and_look_up(&core, &["surfaces/default.surface_c", "sounds/ui/click.sound_c", "prefabs/effects/spark.prefab_c"]);
    assert_eq!(n, 3);
    std::fs::remove_dir_all(&core).unwrap();
}

/// sbox-public's own core (`SBOX_PUBLIC`). With `UNDERBOX_REAL_CORE=<real engine2's game/core>`
/// (local only, never committed) it also checks we compile exactly the GameResources real ships.
#[test]
fn sbox_core_resources_resolve() {
    let Some(sbox) = std::env::var_os("SBOX_PUBLIC") else { return eprintln!("skipped: SBOX_PUBLIC unset") };
    let core = scratch("core_sbox");
    let extensions: Vec<&str> = assets::game_resource::engine_extensions().collect();
    let is_resource = |p: &Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e));
    let src = Path::new(&sbox).join("game/core");
    let sources = files(&src, &|p| is_resource(p));
    for rel in &sources {
        std::fs::create_dir_all(core.join(rel).parent().unwrap()).unwrap();
        std::fs::copy(src.join(rel), core.join(rel)).unwrap();
    }
    let n = compile_and_look_up(
        &core,
        &["surfaces/default.surface_c", "sounds/core.ambient.sound_c", "prefabs/effects/default_tracer.prefab_c", "decals/circle.decal_c"],
    );
    assert_eq!(n, sources.len());
    if let Some(real) = std::env::var_os("UNDERBOX_REAL_CORE") {
        let ours = files(&core, &|p| p.extension().is_some_and(|e| e.to_string_lossy().ends_with("_c")));
        let compiled_resource = |p: &Path| p.to_string_lossy().strip_suffix("_c").is_some_and(|s| is_resource(Path::new(s)));
        // Real's build lowercases some names (templates/), which lookups ignore.
        let lower = |v: Vec<PathBuf>| v.iter().map(|p| p.to_string_lossy().to_lowercase()).collect::<std::collections::BTreeSet<_>>();
        let theirs = files(Path::new(&real), &compiled_resource);
        assert_eq!(lower(ours), lower(theirs), "compiled GameResources differ from real core");
    }
    std::fs::remove_dir_all(&core).unwrap();
}

/// Paths under `root`, relative and sorted, that `keep` accepts.
fn files(root: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, keep, out);
            } else if keep(&path) {
                out.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, keep, &mut out);
    out.sort();
    out
}
