//! Compiles third_party/box2d/src/*.c into a static library (no cmake: the cc crate drives
//! gcc/clang or MSVC directly). SIMD stays at the SSE2/NEON baseline, as Box2D's default build.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/box2d");
    assert!(root.join("include/box2d/box2d.h").exists(), "run `git submodule update --init third_party/box2d`");
    let src = root.join("src");

    let mut sources: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("third_party/box2d/src")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    sources.sort();

    let mut build = cc::Build::new();
    build.include(root.join("include")).include(&src).files(&sources).warnings(false);
    if build.get_compiler().is_like_msvc() {
        build.std("c17");
    } else {
        build.std("gnu17");
    }
    // As Box2D's CMake Release: B2_ASSERT compiles out. Debug keeps it (it traps on a bad id).
    if std::env::var("DEBUG").as_deref() != Ok("true") {
        build.define("NDEBUG", None);
    }
    build.compile("box2d");

    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed={}", root.join("include/box2d").display());
}
