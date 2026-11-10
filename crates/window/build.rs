//! Builds SDL3 from sdl3-src (static, no audio/gpu/render/camera/tray) and links it.
//! sdl3-sys's own build can't take these switches: SDL 3.4 fails configuration on Linux for every
//! X11 extension whose headers are missing, so each one is turned off here unless its header exists.
//! Linux: cmake, libx11-dev and libxext-dev at minimum (X11 itself is loaded at runtime).
//! Windows: cmake and MSVC.

use std::path::{Path, PathBuf};

/// (cmake option, header under the system include dir) for SDL's optional X11 extensions.
const X11_EXTENSIONS: &[(&str, &str)] = &[
    ("SDL_X11_XCURSOR", "X11/Xcursor/Xcursor.h"),
    ("SDL_X11_XDBE", "X11/extensions/Xdbe.h"),
    ("SDL_X11_XFIXES", "X11/extensions/Xfixes.h"),
    ("SDL_X11_XINPUT", "X11/extensions/XInput2.h"),
    ("SDL_X11_XRANDR", "X11/extensions/Xrandr.h"),
    ("SDL_X11_XSCRNSAVER", "X11/extensions/scrnsaver.h"),
    ("SDL_X11_XSHAPE", "X11/extensions/shape.h"),
    ("SDL_X11_XSYNC", "X11/extensions/sync.h"),
    ("SDL_X11_XTEST", "X11/extensions/XTest.h"),
];

const INCLUDE_DIRS: &[&str] = &["/usr/include", "/usr/local/include"];

const WINDOWS_LIBS: &[&str] = &[
    "advapi32", "cfgmgr32", "gdi32", "imm32", "kernel32", "ole32", "oleaut32", "setupapi", "shell32", "user32", "uuid",
    "version", "winmm",
];

fn has_header(header: &str) -> bool {
    INCLUDE_DIRS.iter().any(|dir| Path::new(dir).join(header).exists())
}

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let mut cfg = cmake::Config::new(sdl3_src::SOURCE_DIR);
    // Gotcha: a Debug SDL compiles in SDL_assert, whose default handler stops at an interactive
    // "Abort/Retry/Ignore" prompt on stdin, which would hang a headless server or test run.
    cfg.profile("Release")
        .define("SDL_STATIC", "ON")
        .define("SDL_SHARED", "OFF")
        .define("SDL_TEST_LIBRARY", "OFF")
        .define("SDL_TESTS", "OFF")
        .define("SDL_EXAMPLES", "OFF")
        .define("SDL_CCACHE", "OFF")
        .define("SDL_REVISION", sdl3_src::REVISION)
        .define("SDL_AUDIO", "OFF")
        .define("SDL_GPU", "OFF")
        .define("SDL_RENDER", "OFF")
        .define("SDL_CAMERA", "OFF")
        .define("SDL_TRAY", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
    if target_os == "linux" {
        for (option, header) in X11_EXTENSIONS {
            if !has_header(header) {
                println!("cargo:warning=SDL3 without {option}: {header} not found");
                cfg.define(*option, "OFF");
            }
        }
    }
    let dst = cfg.build();
    println!("cargo:rustc-link-search=native={}", dst.join("lib").display());
    let windows = target_os == "windows";
    println!("cargo:rustc-link-lib=static={}", if windows { "SDL3-static" } else { "SDL3" });
    let mut libs = private_libs(&dst);
    if windows {
        // In case sdl3.pc under MSVC lists them in another form: what SDL3's Windows backends use.
        libs.extend(WINDOWS_LIBS.iter().map(|l| l.to_string()));
        libs.sort();
        libs.dedup();
    }
    for lib in libs {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// System libraries a static SDL3 needs, from the `Libs`/`Libs.private` lines of its sdl3.pc.
fn private_libs(dst: &Path) -> Vec<String> {
    let pc: PathBuf = ["lib", "pkgconfig", "sdl3.pc"].iter().fold(dst.to_path_buf(), |p, c| p.join(c));
    let text = std::fs::read_to_string(&pc).unwrap_or_else(|e| panic!("{}: {e}", pc.display()));
    text.lines()
        .filter_map(|l| l.strip_prefix("Libs:").or_else(|| l.strip_prefix("Libs.private:")))
        .flat_map(str::split_whitespace)
        .filter_map(|t| t.strip_prefix("-l").or_else(|| t.strip_suffix(".lib")))
        .map(|l| l.strip_suffix(".lib").unwrap_or(l))
        .filter(|l| !l.starts_with("SDL3") && !l.contains(['/', '\\', '$']))
        .map(str::to_owned)
        .collect()
}
