#!/bin/sh
# Put the call tap in front of the engine2 in <native-dir> (a game dir's bin/linuxsteamrt64): the
# engine becomes libengine2_target.so and the tap libengine2.so. See docs/differential-tap.md.
# usage: tools/diff/tap-install.sh <native-dir>   (install-engine2.sh and real-game.sh call it when
# UNDERBOX_TAP_DIR is set)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
NATIVE=$1
# see docs/dev-loop.md (incremental)
env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p tap -j4 >&2
TAP="${CARGO_TARGET_DIR:-$ROOT/target}/debug/libengine2_tap.so"
mv -f "$NATIVE/libengine2.so" "$NATIVE/libengine2_target.so"
# Our modules managed loads by name are symlinks to engine2 (install-engine2.sh): keep them on it.
for l in "$NATIVE"/*.so; do
    if [ -L "$l" ] && [ "$(readlink "$l")" = libengine2.so ]; then ln -sfn libengine2_target.so "$l"; fi
done
# Rename, not overwrite (install-engine2.sh).
cp --reflink=auto "$TAP" "$NATIVE/.libengine2.so.$$"
mv -f "$NATIVE/.libengine2.so.$$" "$NATIVE/libengine2.so"
echo "tap-install: $NATIVE/libengine2.so taps libengine2_target.so, logging to ${UNDERBOX_TAP_DIR:-nowhere (UNDERBOX_TAP_DIR unset)}" >&2
