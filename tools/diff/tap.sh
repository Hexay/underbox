#!/bin/sh
# Record every interop call of one dedicated-server run per engine through the call tap, then
# compare the streams (tools/diff/tap.py). See docs/differential-tap.md.
# usage: tools/diff/tap.sh <probe|corpus-name|game-checkout> [both|ours|real|real2] [artifact-root]   (or tools/diff/run.sh tap ...)
#   both   ours, then real REAL_RUNS times (default 2: real2, the second, calibrates real's own noise)
# Env: TICK (default IPhysicsWorld_StepSimulation), SLOTS, CLASSES, TICKS, MAX_MB (default 1024),
#      SECS, UNTIL/GRACE for corpus scenes (default: 'Server is idle', 5 s), N (divergences shown, 20).
# Logs: $UNDERBOX_CACHE/underbox-tap/<worktree>/<name>/<run>/t*.tap; boot output and report.txt in
# target/diff/tap/<name>/.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/cache-dir.sh"
. "$ROOT/tools/diff/env.sh"
SCN=${1:?usage: tools/diff/tap.sh <probe|corpus|game-checkout> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-}
[ -n "$ART" ] || ART=$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
NAME=$(basename -- "$SCN")
OUT=$ROOT/target/diff/tap/$NAME
TAPS=$UNDERBOX_CACHE/underbox-tap/$(basename -- "$ROOT")/$NAME
mkdir -p "$OUT" "$TAPS"
export UNDERBOX_TAP_TICK=${TICK:-IPhysicsWorld_StepSimulation} UNDERBOX_TAP_MAX_MB=${MAX_MB:-1024}
export UNDERBOX_TAP_SLOTS=${SLOTS:-*} UNDERBOX_TAP_CLASSES=${CLASSES:-*} UNDERBOX_TAP_TICKS=${TICKS:-}

one() {
    run=$1
    real=
    case $run in real*) [ -n "$ART" ] || { echo "tap: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; real=$ART ;; esac
    rm -rf "${TAPS:?}/$run"
    echo "== $NAME on $run"
    export UNDERBOX_TAP_DIR="$TAPS/$run" UNDERBOX_REAL_ENGINE2="$real"
    if [ "$SCN" = probe ]; then
        OUT="$OUT/$run" UNTIL='DIFF:END' GRACE=1 sh "$ROOT/tools/boot-server.sh" "${SECS:-150}" "$SBOX" -- \
            +game "$ROOT/tools/diff/probe/.sbproj" > "$OUT/$run.txt" 2>&1 || true
    elif [ -d "$ROOT/corpus/$SCN" ]; then
        OUT="$OUT/$run" UNTIL=${UNTIL-Server is idle} GRACE=${GRACE:-5} sh "$ROOT/tools/boot-server.sh" "${SECS:-90}" "$SBOX" -- \
            +game "$ROOT/corpus/$SCN/.sbproj" > "$OUT/$run.txt" 2>&1 || true
    elif [ -d "$SCN" ]; then
        OUT="$OUT/$run" SECS=${SECS:-180} sh "$ROOT/tools/game.sh" "$SCN" > "$OUT/$run.txt" 2>&1 || true
    else
        echo "tap: no scenario, corpus or checkout '$SCN'" >&2; exit 2
    fi
    grep -hE '^tap(-install)?:' "$OUT/$run.txt" "$OUT/$run/server.log" 2>/dev/null | head -n 3 || true
    echo "$run: $(cat "$TAPS/$run"/t*.tap 2>/dev/null | wc -l) records, $(du -sh "$TAPS/$run" | cut -f1)"
}

case $WHICH in
    both)
        one ours
        i=1
        while [ "$i" -le "${REAL_RUNS:-2}" ]; do one "real$([ "$i" = 1 ] || echo "$i")"; i=$((i + 1)); done ;;
    ours|real|real[0-9]) one "$WHICH" ;;
    *) echo "tap: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
[ -d "$TAPS/ours" ] && [ -d "$TAPS/real" ] || exit 0
CAL=
[ -d "$TAPS/real2" ] && CAL="--calibrate $TAPS/real2"
# shellcheck disable=SC2086
python3 -I "$ROOT/tools/diff/tap.py" "$TAPS/ours" "$TAPS/real" $CAL -n "${N:-20}" > "$OUT/report.txt"
head -n 30 "$OUT/report.txt"
echo "tap: full report in $OUT/report.txt"
