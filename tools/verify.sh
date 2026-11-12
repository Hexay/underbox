#!/bin/sh
# Merge gate: crate tests, workspace clippy, coverage, boot checks, golden frames.
# usage: [VERIFY_JOBS=n] tools/verify.sh [crate ...]   (default: every crate with tests that boots or renders)
# An engine2 build and the GPU-heavy crate tests (render, engine2) first, then two lanes side by side:
# cargo (the other crate tests, clippy, coverage, one at a time) and boots (match, map, the golden
# frames; VERIFY_JOBS at once, default 4). Once a step fails, steps not yet started are skipped;
# every failure is reported. Logs: target/verify/<step>.log. See docs/dev-loop.md (merge gate).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
LOGS=$ROOT/target/verify
STEP="$ROOT/tools/verify-step.sh"
crates=${*:-"gns assets world anim physics render shaders engine2"}
gpu_crates=
lane_crates=
for c in $crates; do
    case $c in render|engine2) gpu_crates="$gpu_crates $c" ;; *) lane_crates="$lane_crates $c" ;; esac
done
t0=$(date +%s)
rm -rf "$LOGS"
mkdir -p "$LOGS"

# see docs/dev-loop.md (incremental)
first_steps() {
    echo "build env -u CARGO_INCREMENTAL cargo build -q -p engine2 -j4"
    for c in $gpu_crates; do echo "test-$c sh tools/test.sh $c"; done
}
cargo_steps() {
    for c in $lane_crates; do echo "test-$c sh tools/test.sh $c"; done
    echo "clippy env -u CARGO_INCREMENTAL cargo clippy -q --workspace --all-targets -j4 -- -D warnings"
    echo "coverage env -u CARGO_INCREMENTAL cargo run -q -p engine2 --bin coverage -j4"
}
boot_steps() {
    echo "match sh tools/match.sh"
    echo "map sh tools/map.sh"
    for c in showroom mapworld skinned outdoor fog hud glass particles decals; do
        n=frame-$c
        [ "$c" = showroom ] && n=frame
        echo "$n env CORPUS=$c sh tools/client-frame.sh"
        [ "$c" = mapworld ] || echo "$n-managed env CORPUS=$c MANAGED=1 sh tools/client-frame.sh"
    done
}
report() {
    fail=0
    for n in "$@"; do
        [ -f "$LOGS/$n.rc" ] || { echo "== $n: FAIL (never ran)"; fail=1; continue; }
        read -r rc secs < "$LOGS/$n.rc"
        case $rc in
            0) printf '== %s: %s (%ss)\n' "$n" "$(tail -n 1 "$LOGS/$n.log")" "$secs" ;;
            skip) echo "== $n: skipped" ;;
            *) echo "== $n: FAIL (exit $rc, ${secs}s)"; tail -n 20 "$LOGS/$n.log"; fail=1 ;;
        esac
    done
    return $fail
}

first_steps | xargs -L1 sh "$STEP" "$LOGS"
report $(first_steps | cut -d' ' -f1) || { echo "RESULT: FAIL (logs in $LOGS)"; exit 1; }
# Boots install this copy instead of running cargo, which would wait on the cargo lane's lock.
cp --reflink=auto "${CARGO_TARGET_DIR:-$ROOT/target}/debug/libengine2.so" "$LOGS/libengine2.so"
export UNDERBOX_ENGINE2="$LOGS/libengine2.so"

cargo_steps | xargs -L1 sh "$STEP" "$LOGS" &
lane=$!
boot_steps | xargs -L1 -P "${VERIFY_JOBS:-4}" sh "$STEP" "$LOGS"
wait $lane || true
status=0
report $(cargo_steps | cut -d' ' -f1) $(boot_steps | cut -d' ' -f1) || status=1
echo "== wall: $(($(date +%s) - t0))s"
[ $status -eq 0 ] || { echo "RESULT: FAIL (logs in $LOGS)"; exit 1; }
echo "RESULT: PASS"
