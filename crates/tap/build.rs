//! Link the tap against an empty `libengine2_target.so` so the library records it as DT_NEEDED
//! (with RUNPATH `$ORIGIN`). The loader then maps the real target next to the tap, and every export
//! the tap doesn't define resolves in the target: see docs/differential-tap.md.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let stub = out.join("libengine2_target.so");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let status = Command::new(&cc)
        .args(["-shared", "-nostdlib", "-Wl,-soname,libengine2_target.so", "-x", "c", "/dev/null", "-o"])
        .arg(&stub)
        .status()
        .unwrap_or_else(|e| panic!("tap: running {cc}: {e}"));
    assert!(status.success(), "tap: {cc} failed to build the target stub");
    println!("cargo:rustc-link-search=native={}", out.display());
    for arg in ["-Wl,--no-as-needed", "-lengine2_target", "-Wl,-rpath,$ORIGIN"] {
        println!("cargo:rustc-cdylib-link-arg={arg}");
    }
}
