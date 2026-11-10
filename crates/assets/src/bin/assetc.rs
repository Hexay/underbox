//! Compile a project's content ahead of time (CI, packaging): `assetc <project Assets dir>...`.
//! Exits non-zero if anything failed. The engine runs the same compile when it mounts a project.

use std::process::ExitCode;

fn main() -> ExitCode {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        eprintln!("usage: assetc <Assets dir>...");
        return ExitCode::from(2);
    }
    let mut failed = false;
    for dir in dirs {
        let r = assets::project::compile(dir.as_ref());
        for (src, e) in &r.failed {
            eprintln!("error: {}: {e}", src.display());
        }
        println!("{dir}: {} compiled, {} up to date, {} failed", r.compiled.len(), r.up_to_date, r.failed.len());
        failed |= !r.failed.is_empty();
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
