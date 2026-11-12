#!/bin/sh
# Object-level state differential: dump every GameObject, component and physics body at fixed ticks on
# our engine2 and on Facepunch's, then rank what differs (docs/differential-state.md).
# usage: tools/diff/state.sh <corpus|game-checkout> [both|ours|real] [artifact-root]   (or tools/diff/run.sh state ...)
# Env: TICKS=1,5,25,50,100,200 EVERY=0 LAST=0 (StateDump's props), SCENE=scenes/x.scene, SIDE=server|client,
# NOISE=1 (real runs twice; properties that differ between those are ignored), CONTENT=source (no real
# core content seeded into ours), SECS=240.
# Out: target/diff/state/<name>/{ours,real,real2}.jsonl, their boot dirs, report.txt.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
. "$ROOT/tools/cache-dir.sh"
SRC=${1:?usage: tools/diff/state.sh <corpus|game-checkout> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')}
SIDE=${SIDE:-server}
SECS=${SECS:-240}
[ -d "$SRC" ] || SRC=$ROOT/corpus/$SRC
[ -d "$SRC" ] || { echo "state: no corpus or checkout '$1'" >&2; exit 2; }
SRC=$(cd -- "$SRC" && pwd)
NAME=$(basename -- "$SRC")
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
OUT=$ROOT/target/diff/state/$NAME
[ "$SIDE" = server ] || OUT=$OUT-$SIDE
mkdir -p "$OUT" "$UNDERBOX_GAME_COPIES"

# Synced fresh for every invocation: real's boot compiles into the copy (tools/diff/real-game.sh).
prepare() {
    copy=$UNDERBOX_GAME_COPIES/$NAME-state-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
    sh "$ROOT/tools/game/copy.sh" "$SRC" "$copy"
    python3 -I "$ROOT/tools/game/prepare.py" "$copy" "${SCENE:-}" "$ROOT/tools/diff/StateDump.cs" --stub-lobby \
        --prop "Ticks=${TICKS:-1,5,25,50,100,200}" --prop "Every=${EVERY:-0}" --prop "Last=${LAST:-0}"
    for f in "$ROOT"/tools/diff/StateDump?*.cs; do cp "$f" "$copy/Code/"; done
}

# boot <run-name> <artifact-root or empty for ours>
boot() {
    run=$1 real=$2 dir=$OUT/$1 seed=
    # A compile error in the injected code would otherwise idle to the timeout.
    until='STATE:END|Error \| .*StateDump'
    echo "== $NAME on $run ($SIDE)"
    PROJECT=$(prepare)
    [ -n "$real" ] || [ "${CONTENT:-real}" = source ] || [ -z "$ART" ] || seed=$(sh "$ROOT/tools/diff/seed.sh" "$ART")
    if [ "$SIDE" = server ]; then
        UNDERBOX_REAL_ENGINE2=$real SEED=$seed OUT=$dir UNTIL="$until" GRACE=1 \
            sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- +game "$PROJECT" > "$OUT/$run.txt" 2>&1 || true
        log=$dir/server.log
    else
        UNDERBOX_STANDIN_FRIENDS=1 UNDERBOX_REAL_ENGINE2=$real SEED=$seed OUT=$dir UNTIL="$until" GRACE=1 \
            sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$PROJECT" > "$OUT/$run.txt" 2>&1 || true
        log=$dir/client.log
    fi
    if grep '^FAIL: the game downloaded' "$OUT/$run.txt"; then exit 4; fi
    grep -o 'STATE:{.*' "$log" | cut -c7- > "$OUT/$run.jsonl" || true
    echo "$run: $(wc -l < "$OUT/$run.jsonl") lines; $(grep -c 'STATE:TICK' "$log" || true) dumps; $(grep -q 'STATE:END' "$log" && echo ended || echo 'NO STATE:END')"
}

need_real() { [ -n "$ART" ] || { echo "state: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; }
case $WHICH in
    both) need_real; boot ours ""; boot real "$ART"; [ "${NOISE:-1}" = 0 ] || boot real2 "$ART" ;;
    ours) boot ours "" ;;
    real) need_real; boot real "$ART"; [ "${NOISE:-1}" = 0 ] || boot real2 "$ART" ;;
    *) echo "state: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
[ -s "$OUT/ours.jsonl" ] && [ -s "$OUT/real.jsonl" ] || exit 0
noise=
[ "${NOISE:-1}" = 0 ] || [ ! -s "$OUT/real2.jsonl" ] || noise="--noise $OUT/real2.jsonl"
# shellcheck disable=SC2086
python3 -I "$ROOT/tools/diff/state.py" "$OUT/ours.jsonl" "$OUT/real.jsonl" $noise | tee "$OUT/report.txt"
