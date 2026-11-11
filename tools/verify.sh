#!/bin/sh
# Merge gate: crate tests, workspace clippy, boot checks, golden frames, coverage. Stops at the first failure.
# usage: tools/verify.sh [crate ...]   (default: every crate with tests that boots or renders)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
step() { printf '== %s: ' "$1"; shift; out=$("$@" 2>&1) || { echo FAIL; printf '%s\n' "$out" | tail -n 20; exit 1; }; printf '%s\n' "$out" | tail -n 1; }
crates=${*:-"gns assets world anim physics render shaders engine2"}
for c in $crates; do step "test $c" sh "$ROOT/tools/test.sh" "$c"; done
step clippy env -u CARGO_INCREMENTAL cargo clippy -q --manifest-path "$ROOT/Cargo.toml" --workspace --all-targets -j4 -- -D warnings
step match sh "$ROOT/tools/match.sh"
step map sh "$ROOT/tools/map.sh"
step frame sh "$ROOT/tools/client-frame.sh"
step frame-mapworld env CORPUS=mapworld sh "$ROOT/tools/client-frame.sh"
step frame-managed env MANAGED=1 sh "$ROOT/tools/client-frame.sh"
step frame-skinned env CORPUS=skinned sh "$ROOT/tools/client-frame.sh"
step frame-skinned-managed env CORPUS=skinned MANAGED=1 sh "$ROOT/tools/client-frame.sh"
step frame-outdoor env CORPUS=outdoor sh "$ROOT/tools/client-frame.sh"
step frame-outdoor-managed env CORPUS=outdoor MANAGED=1 sh "$ROOT/tools/client-frame.sh"
step frame-fog env CORPUS=fog sh "$ROOT/tools/client-frame.sh"
step frame-fog-managed env CORPUS=fog MANAGED=1 sh "$ROOT/tools/client-frame.sh"
# hud's first UI frame (~#15) stalls lavapipe for up to ~30 s, so wait for frames past it.
HUD="CORPUS=hud UNTIL=frame:.GameWindow.#40. GRACE=1"
step frame-hud env $HUD sh "$ROOT/tools/client-frame.sh"
step frame-hud-managed env $HUD MANAGED=1 sh "$ROOT/tools/client-frame.sh"
step coverage cargo run -q --manifest-path "$ROOT/Cargo.toml" -p engine2 --bin coverage -j4
echo "RESULT: PASS"
