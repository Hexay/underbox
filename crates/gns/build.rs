//! Builds third_party/GameNetworkingSockets (static, no ICE) and src/shim.cpp.
//! Linux: cmake, protobuf (protoc + libprotobuf) and OpenSSL dev packages.
//! Windows: protobuf from vcpkg (`VCPKG_ROOT`, triplet x64-windows-static-md); crypto is BCrypt.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/GameNetworkingSockets");
    assert!(root.join("CMakeLists.txt").exists(), "run `git submodule update --init third_party/GameNetworkingSockets`");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");

    let mut cfg = cmake::Config::new(&root);
    cfg.define("BUILD_STATIC_LIB", "ON")
        .define("BUILD_SHARED_LIB", "OFF")
        .define("BUILD_EXAMPLES", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("BUILD_TOOLS", "OFF")
        .define("ENABLE_ICE", "OFF")
        .build_target("GameNetworkingSockets_s");
    if windows {
        let vcpkg = PathBuf::from(std::env::var("VCPKG_ROOT").expect("set VCPKG_ROOT (protobuf from vcpkg)"));
        cfg.define("USE_CRYPTO", "BCrypt")
            .define("CMAKE_TOOLCHAIN_FILE", vcpkg.join("scripts/buildsystems/vcpkg.cmake"))
            .define("VCPKG_TARGET_TRIPLET", "x64-windows-static-md")
            .define("MSVC_CRT_STATIC", "OFF");
        println!("cargo:rustc-link-search=native={}", vcpkg.join("installed/x64-windows-static-md/lib").display());
    } else {
        cfg.define("USE_CRYPTO", "OpenSSL");
    }
    let dst = cfg.build();
    // Multi-config generators (MSVC) put the library in a per-profile subdirectory.
    let lib = dst.join("build/src");
    for dir in [lib.clone(), lib.join("Release"), lib.join("Debug"), lib.join("RelWithDebInfo")] {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    println!("cargo:rustc-link-lib=static=GameNetworkingSockets_s");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(root.join("include"))
        .define("STEAMNETWORKINGSOCKETS_STATIC_LINK", None)
        .file("src/shim.cpp")
        .compile("gns_shim");

    let libs: &[&str] = if windows {
        &["libprotobuf", "ws2_32", "crypt32", "winmm", "iphlpapi", "bcrypt", "advapi32"]
    } else {
        &["protobuf", "ssl", "crypto", "stdc++", "pthread"]
    };
    for l in libs {
        println!("cargo:rustc-link-lib=dylib={l}");
    }
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
}
