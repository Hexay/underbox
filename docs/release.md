# Release builds (Linux)

| Step | Command |
| --- | --- |
| Dist build | `tools/dist.sh [out]`: release libengine2 in the `bin/linuxsteamrt64` layout plus `manifest.txt`, `LICENSE`, `NOTICE` and `licenses/` (each `third_party` submodule's licence), in `dist/` |
| Dependency check | `tools/dist-check.sh <libengine2.so>` (dist.sh runs it; `DIST_GLIBC_MAX=2.35` also caps glibc) |
| Boot a dist build | `UNDERBOX_ENGINE2=$PWD/dist/bin/linuxsteamrt64/libengine2.so tools/match.sh` (any boot script) |

The released artifact is CI's `dist` job (`libengine2-linux-x64.tar.gz`), built in an `ubuntu:22.04`
container. A local `dist.sh` produces the same layout, but carries this host's glibc (see below).

## Dependency policy

`dist-check.sh` fails on any `DT_NEEDED` outside this list, and on an undefined `snd_*` symbol.

| Library | How | Why |
| --- | --- | --- |
| glibc (`libc`, `libm`, `ld-linux`; `libdl`/`libpthread`/`librt` before 2.34) | dynamic | The baseline; can't be static in a dlopened library |
| `libgcc_s.so.1`, `libstdc++.so.6` | dynamic | .NET's coreclr links libstdc++, so it is in every process that loads us; building on the baseline distro bounds `GLIBCXX` |
| `libcrypto.so.3` (OpenSSL 3, GNS's crypto) | dynamic | Stable soname, needs only `OPENSSL_3.0.0`, and .NET on Linux requires OpenSSL anyway; static would freeze security fixes. `libssl` is not needed (`--as-needed` drops it) |
| protobuf | static | `libprotobuf.so.N` changes soname every release |
| zlib, zstd | gone | Only came in through the distro libprotobuf; ours is built without zlib |
| ALSA | dlopened | See below |

## glibc baseline: 2.35

Symbol versions bind to the newest version in the libc a build links against, for Rust and C alike,
so the baseline is the build host's glibc. On Ubuntu 26.04 (glibc 2.43) libengine2 needs 2.43:
`acosf`, `sqrtf` and other float math got new versions in 2.43, and C compiled with 2.38+ headers
calls `__isoc23_strtol` and friends. CI's `dist` job builds on Ubuntu 22.04 and sets
`DIST_GLIBC_MAX=2.35`, so the release loads on glibc 2.35+ with OpenSSL 3: Ubuntu 22.04+, Debian 12+,
Fedora 36+, Arch. Not RHEL 9 (2.34) or Steam Runtime 3 "sniper" (2.31, OpenSSL 1.1).

Rejected: `cargo zigbuild --target x86_64-unknown-linux-gnu.2.34` (any baseline from any host)
hides the system X11 and OpenSSL headers behind zig's sysroot, so SDL3 and GNS fail to configure.

## Static protobuf

`UNDERBOX_STATIC_PROTOBUF=1` (dist.sh sets it) makes crates/gns/build.rs build the
third_party/protobuf submodule (v21.12, the last release without Abseil; it matches Ubuntu's protoc
3.21.12) with PIC, plus its protoc for GNS's generated code. Distro `libprotobuf.a` is not PIC, so
it can't go into the cdylib. Cold cost: about 38 s wall at `-j4` (136 CPU-s), once per build dir;
sccache serves it in CI. Dev builds keep the system lib.

## ALSA without a link dependency

One libengine2 serves a dedicated server with no ALSA and a client with sound. alsa-sys has no
dlopen mode, so .cargo/config.toml overrides its build script (`links = "alsa"`) to link nothing,
and crates/audio/src/alsa_dl defines each `snd_*` symbol as a `jmp` through a slot that `load()`
fills from `dlopen("libasound.so.2")`. A jump needs no signatures and keeps variadic calls intact.
`device::host()` calls `load()` first; without libasound, device listing is empty and opening fails,
so engine2 uses the null device. The trampolines are local (not exported) and gc'd when unused.

- Adding a cpal/alsa version: a newly referenced function shows up as an undefined `snd_*` in
  dist-check; add it to `alsa_dl/symbols.rs` (the test checks every name resolves in libasound).
- The override is per triple (x86_64-unknown-linux-gnu, matching alsa_dl's `cfg`); other Linux
  targets link libasound normally.
- Rejected: a server build without `audio-device` (two artifacts), and a separate audio plugin
  `.so` (a C ABI around the device API, two copies of std).
