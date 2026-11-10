//! Builds third_party/GameNetworkingSockets (static, OpenSSL crypto, no ICE) and src/shim.cpp.
//! Needs cmake, protobuf (protoc + libprotobuf) and OpenSSL dev packages.
// TODO(windows): protobuf/OpenSSL via vcpkg.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/GameNetworkingSockets");
    assert!(root.join("CMakeLists.txt").exists(), "run `git submodule update --init third_party/GameNetworkingSockets`");

    let dst = cmake::Config::new(&root)
        .define("BUILD_STATIC_LIB", "ON")
        .define("BUILD_SHARED_LIB", "OFF")
        .define("BUILD_EXAMPLES", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("BUILD_TOOLS", "OFF")
        .define("ENABLE_ICE", "OFF")
        .define("USE_CRYPTO", "OpenSSL")
        .build_target("GameNetworkingSockets_s")
        .build();
    let lib = dst.join("build/src");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=GameNetworkingSockets_s");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(root.join("include"))
        .define("STEAMNETWORKINGSOCKETS_STATIC_LINK", None)
        .file("src/shim.cpp")
        .compile("gns_shim");

    for l in ["protobuf", "ssl", "crypto", "stdc++", "pthread"] {
        println!("cargo:rustc-link-lib=dylib={l}");
    }
    println!("cargo:rerun-if-changed=src/shim.cpp");
}
