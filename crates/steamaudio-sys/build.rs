//! Compiles the part of Steam Audio's core (third_party/steam-audio) the binaural effect needs,
//! with PFFFT (third_party/pffft) for its FFT, as Steam Audio's own build does without Intel IPP.
//! No cmake: SOFA loading (libmysofa), FlatBuffers and the simulation API stay out.

use std::path::PathBuf;

const CORE: &[&str] = &[
    "ambisonics_panning_effect.cpp",
    "array_math.cpp",
    "audio_buffer.cpp",
    "binaural_effect.cpp",
    "hrtf.cpp",
    "hrtf_database.cpp",
    "hrtf_map.cpp",
    "hrtf_map_factory.cpp",
    "log.cpp",
    "math_functions.cpp",
    "memory_allocator.cpp",
    "overlap_add_convolution_effect.cpp",
    "panning_effect.cpp",
    "pffft_fft.cpp",
    "sh.cpp",
    "sh/spherical_harmonics.cc",
    "speaker_layout.cpp",
    "window_function.cpp",
];

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let third = manifest.join("../../third_party");
    let core = third.join("steam-audio/core/src/core");
    let pffft = third.join("pffft");
    assert!(core.join("binaural_effect.cpp").exists(), "run `git submodule update --init third_party/steam-audio`");
    assert!(pffft.join("pffft.c").exists(), "run `git submodule update --init third_party/pffft`");
    let release = std::env::var("DEBUG").as_deref() != Ok("true");

    // Optimized in dev builds too: it runs per voice per block on the mixing thread.
    let mut c = cc::Build::new();
    c.opt_level(2).files([pffft.join("pffft.c"), pffft.join("pffft_common.c")]).include(&pffft).warnings(false);
    if release {
        c.define("NDEBUG", None);
    }
    c.compile("pffft");

    let mut cpp = cc::Build::new();
    cpp.cpp(true).opt_level(2).include(&core).include(&pffft).define("IPL_DISABLE_SOFA", None).warnings(false);
    cpp.files(CORE.iter().map(|f| core.join(f))).file(manifest.join("shim/shim.cpp"));
    let prelude = manifest.join("shim/prelude.h");
    if cpp.get_compiler().is_like_msvc() {
        cpp.flag("/std:c++14").flag("/EHsc").flag(format!("/FI{}", prelude.display()));
        for d in ["NOMINMAX", "WIN32_LEAN_AND_MEAN", "_CRT_SECURE_NO_WARNINGS", "_USE_MATH_DEFINES"] {
            cpp.define(d, None);
        }
    } else {
        cpp.flag("-std=c++14").flag("-include").flag(prelude.display().to_string());
    }
    if release {
        cpp.define("NDEBUG", None);
    }
    cpp.compile("steamaudio");

    for p in [core.as_path(), pffft.as_path(), &manifest.join("shim")] {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}
