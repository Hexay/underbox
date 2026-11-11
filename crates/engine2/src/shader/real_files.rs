//! Real `.shader_c` files through our reader, when `UNDERBOX_REAL_SHADERS` lists folders of them
//! (`:`-separated). Skipped otherwise: none are or may be in this repository.

use super::compiled;
use super::*;

fn find_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "shader_c") {
            out.push(p);
        }
    }
}

#[test]
fn real_shader_files_read() {
    let Ok(dirs) = std::env::var("UNDERBOX_REAL_SHADERS") else { return eprintln!("skipped: UNDERBOX_REAL_SHADERS unset") };
    let mut files = Vec::new();
    dirs.split(':').for_each(|d| find_files(Path::new(d), &mut files));
    files.sort();
    assert!(!files.is_empty(), "no .shader_c under {dirs}");
    if let Some(core) = crate::render::shading::tests::core_dir() {
        crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    }
    dirs.split(':').for_each(|d| crate::fs::add_search_path("GAME", d, true));
    let mut failed = 0;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(f).unwrap();
        let summary = match compiled::read(&bytes) {
            Err(e) if e == "no SPRV block" => format!("skipped, no SPIR-V program (blocks: DXBC only): {e}"),
            Err(e) => {
                failed += 1;
                format!("read failed: {e}")
            }
            Ok(l) if l.embedded.is_empty() => format!("no embedded source; programs {:?}", l.programs),
            Ok(l) => {
                let v = Vfx::new(name.clone(), None);
                match load::apply(&v, &bytes, Some(f.as_path()), 0) {
                    Err(e) => {
                        failed += 1;
                        format!("embedded source failed: {e}")
                    }
                    Ok(()) => {
                        let s = v.shader().unwrap();
                        let stages: Vec<String> = s.programs.iter().map(|p| format!("{:?}", p.stage)).collect();
                        let compiled: Vec<String> = s
                            .programs
                            .iter()
                            .map(|p| {
                                let r = compile::compile_combo(&s, p.stage as i64, 0, 0);
                                format!("{:?}:{}", p.stage, if r.failed { format!("FAILED {}", r.output.lines().find(|l| l.contains("error")).unwrap_or("")) } else { "ok".to_string() })
                            })
                            .collect();
                        format!("programs {:?} stages {stages:?} features {} compile {compiled:?}", l.programs, s.features.len())
                    }
                }
            }
        };
        println!("{name}: {summary}");
    }
    assert_eq!(failed, 0, "{failed} files failed to read or parse");
}
