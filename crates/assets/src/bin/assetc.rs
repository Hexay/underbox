//! Compile a project's content ahead of time (CI, packaging): `assetc [--materials] <project Assets dir>...`.
//! Exits non-zero if anything failed. The engine runs the same compile when it mounts a project;
//! `--materials` also compiles `.vmat` sources, which only engines without a source reader need.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut dirs: Vec<String> = std::env::args().skip(1).collect();
    let materials = dirs.first().is_some_and(|a| a == "--materials");
    if materials {
        dirs.remove(0);
    }
    if dirs.is_empty() {
        eprintln!("usage: assetc [--materials] <Assets dir>...");
        return ExitCode::from(2);
    }
    let mut failed = false;
    for dir in dirs {
        let mut r = assets::project::compile(dir.as_ref());
        if materials {
            let m = assets::project::compile_materials(dir.as_ref());
            r.compiled.extend(m.compiled);
            r.up_to_date += m.up_to_date;
            r.failed.extend(m.failed);
        }
        for (src, e) in &r.failed {
            eprintln!("error: {}: {e}", src.display());
        }
        println!("{dir}: {} compiled, {} up to date, {} failed", r.compiled.len(), r.up_to_date, r.failed.len());
        failed |= !r.failed.is_empty();
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
