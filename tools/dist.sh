#!/bin/sh
# Release libengine2 for other Linux machines: the bin/linuxsteamrt64 layout tools/install-engine2.sh
# writes, plus manifest.txt (version, git hash, dependencies), in <out> (default dist/). See docs/release.md.
# usage: tools/dist.sh [out]   (DIST_GLIBC_MAX=2.35 fails the build on newer glibc symbol versions)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
OUT=${1:-$ROOT/dist}
SO=${CARGO_TARGET_DIR:-$ROOT/target}/release/libengine2.so

sh "$ROOT/tools/submodules.sh"
# see docs/dev-loop.md (incremental)
UNDERBOX_STATIC_PROTOBUF=1 env -u CARGO_INCREMENTAL cargo build -q --release --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4
deps=$(sh "$ROOT/tools/dist-check.sh" "$SO") || { echo "$deps"; exit 1; }

rm -rf "$OUT"
UNDERBOX_ENGINE2=$SO sh "$ROOT/tools/install-engine2.sh" "$OUT"
dirty=$(git -C "$ROOT" diff --quiet HEAD -- || echo "-dirty")
{
    echo "version: $(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/crates/engine2/Cargo.toml")"
    echo "git: $(git -C "$ROOT" rev-parse HEAD)$dirty"
    echo "built: $(date -u +%Y-%m-%dT%H:%M:%SZ) on $(. /etc/os-release && echo "$PRETTY_NAME"), $(rustc --version)"
    echo "$deps" | grep -v '^RESULT'
    echo "files:"
    (cd "$OUT" && find bin -type f -exec sha256sum {} + | sort -k2 | sed 's/^/  /')
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"
echo "RESULT: PASS ($OUT)"
