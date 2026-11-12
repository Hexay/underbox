#!/bin/sh
# Build the Steam stand-in that real engine2 clients get LD_PRELOADed (tools/diff/standin/, docs/interop.md)
# and print its path. Headers: the Steamworks SDK steamworks-sys vendors (already in Cargo.lock).
# usage: tools/diff/standin.sh                 build and print the stand-in
#        tools/diff/standin.sh --shim <bin dir>  in a real run copy's bin/linuxsteamrt64, rename
#            libsteam_api.so to libsteam_api_fp.so and put standin/flat.cpp's shim in its place
#            (UNDERBOX_STANDIN_FRIENDS runs, docs/differential-audio.md)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
OUT=${CARGO_TARGET_DIR:-$ROOT/target}/standin
SO=$OUT/libsteam_standin.so
SRC=$ROOT/tools/diff/standin

cxx() {
    SYS=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 --all-features |
        jq -r '.packages[] | select(.name == "steamworks-sys") | .manifest_path' | head -n 1)
    [ -n "$SYS" ] || { echo "standin: steamworks-sys not in the workspace" >&2; exit 3; }
    c++ -std=c++17 -O1 -fPIC -shared -Wall -Wno-unused-parameter -I "$(dirname -- "$SYS")/lib/steam/public" "$@"
}

if [ "${1:-}" = --shim ]; then
    BIN=${2:?usage: tools/diff/standin.sh --shim <bin dir>}
    [ -f "$BIN/libsteam_api_fp.so" ] || mv "$BIN/libsteam_api.so" "$BIN/libsteam_api_fp.so"
    rm -f "$BIN/libsteam_api.so"
    # Linking the original would record its soname (libsteam_api.so, ourselves): link an empty
    # library named after the renamed file instead.
    STUB=$(mktemp -d)
    trap 'rm -rf "$STUB"' EXIT
    cxx -o "$STUB/libsteam_api_fp.so" -x c++ /dev/null -Wl,-soname,libsteam_api_fp.so
    cxx -o "$BIN/libsteam_api.so" "$SRC/flat.cpp" -Wl,-soname,libsteam_api.so -Wl,-rpath,'$ORIGIN' \
        -L "$STUB" -Wl,--no-as-needed -l:libsteam_api_fp.so
    exit 0
fi

if [ ! -f "$SO" ] || [ "$SRC/standin.cpp" -nt "$SO" ] || [ "$SRC/friends.cpp" -nt "$SO" ]; then
    mkdir -p "$OUT"
    cxx -o "$SO.$$" "$SRC/standin.cpp" "$SRC/friends.cpp" -ldl
    mv -f "$SO.$$" "$SO"
fi
echo "$SO"
