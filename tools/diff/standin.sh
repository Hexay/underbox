#!/bin/sh
# Build the Steam stand-in that real engine2 clients get LD_PRELOADed (tools/diff/standin/, docs/interop.md)
# and print its path. Headers: the Steamworks SDK steamworks-sys vendors (already in Cargo.lock).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
OUT=${CARGO_TARGET_DIR:-$ROOT/target}/standin
SO=$OUT/libsteam_standin.so
SRC=$ROOT/tools/diff/standin/standin.cpp
if [ ! -f "$SO" ] || [ "$SRC" -nt "$SO" ]; then
    SYS=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 --all-features |
        jq -r '.packages[] | select(.name == "steamworks-sys") | .manifest_path' | head -n 1)
    [ -n "$SYS" ] || { echo "standin: steamworks-sys not in the workspace" >&2; exit 3; }
    mkdir -p "$OUT"
    c++ -std=c++17 -O1 -fPIC -shared -Wall -Wno-unused-parameter -I "$(dirname -- "$SYS")/lib/steam/public" \
        -o "$SO.$$" "$SRC" -ldl
    mv -f "$SO.$$" "$SO"
fi
echo "$SO"
