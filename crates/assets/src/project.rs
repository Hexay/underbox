//! Compile a content folder (a project's Assets, or the engine's core): every source with a
//! compiler gets its `<file>_c` beside it, when that output is missing or older than the source.

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

/// Compile the GameResources under `root` that are stale. The engine runs this at mount.
pub fn compile(root: &Path) -> Report {
    let extensions: Vec<&str> = game_resource::engine_extensions().collect();
    compile_matching(root, |ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)), |text, rel| Ok(game_resource::compile(text, rel, GAME_RESOURCE_VERSION)))
}

/// Compile the stale `.vmat` sources under `root`. Not run at mount (our engine reads sources);
/// for engines that only load compiled materials, such as real engine2 (docs/differential.md).
pub fn compile_materials(root: &Path) -> Report {
    compile_matching(
        root,
        |ext| ext.eq_ignore_ascii_case("vmat"),
        |text, rel| {
            let src = crate::material::from_vmat(text).map_err(|e| format!("{e:?}"))?;
            Ok(crate::compiled::material::write(&crate::compiled::material::from_source(rel, &src)))
        },
    )
}

fn compile_matching(root: &Path, wanted: impl Fn(&str) -> bool, compiler: impl Fn(&str, &str) -> Result<Vec<u8>, String>) -> Report {
    let mut report = Report::default();
    let mut sources = Vec::new();
    walk(root, &mut sources);
    for src in sources {
        let Some(ext) = src.extension().and_then(|e| e.to_str()) else { continue };
        if !wanted(ext) {
            continue;
        }
        let out = PathBuf::from(format!("{}_c", src.display()));
        if is_fresh(&src, &out) {
            report.up_to_date += 1;
            continue;
        }
        match compile_one(root, &src, &out, &compiler) {
            Ok(()) => report.compiled.push(out),
            Err(e) => report.failed.push((src, e)),
        }
    }
    report
}

fn compile_one(root: &Path, src: &Path, out: &Path, compiler: &dyn Fn(&str, &str) -> Result<Vec<u8>, String>) -> Result<(), String> {
    let text = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
    let rel = src.strip_prefix(root).unwrap_or(src).to_string_lossy().replace('\\', "/");
    // Write then rename: another process sharing the content folder may be reading `out` right now.
    let tmp = PathBuf::from(format!("{}.{}.tmp", out.display(), std::process::id()));
    std::fs::write(&tmp, compiler(&text, &rel)?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
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

    #[test]
    fn compiles_materials_as_real_lays_them_out() {
        let dir = std::env::temp_dir().join(format!("assets-project-vmat-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("materials")).unwrap();
        let vmat = "\"Layer0\"\n{\n\t\"shader\" \"shaders/complex.shader\"\n\t\"F_TRANSLUCENT\" \"1\"\n\t\"g_vColorTint\" \"[0.3 0.7 1 0]\"\n\t\"g_flOpacityScale\" \"0.4\"\n}\n";
        std::fs::write(dir.join("materials/glass.vmat"), vmat).unwrap();
        assert_eq!(compile_materials(&dir).compiled.len(), 1);
        let m = crate::compiled::material::read(&std::fs::read(dir.join("materials/glass.vmat_c")).unwrap()).unwrap();
        assert_eq!((m.name.as_str(), m.shader.as_str()), ("materials/glass.vmat", "shaders/complex.shader"));
        assert_eq!(m.ints, vec![("F_TRANSLUCENT".to_owned(), 1)]);
        assert_eq!(m.floats, vec![("g_flOpacityScale".to_owned(), 0.4)]);
        assert_eq!(m.vectors, vec![("g_vColorTint".to_owned(), [0.3, 0.7, 1.0, 0.0])]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
