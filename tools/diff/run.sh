#!/bin/sh
# Run one server-side scenario on our engine2 and on Facepunch's, then rank the divergences.
# usage: tools/diff/run.sh <probe|orbmatch|sweeper|zoo> [both|ours|real] [tol=0.5]
# Logs: target/diff/<scenario>/{ours,real}.log; report: target/diff/<scenario>/report.txt.
# Real engine2 comes from UNDERBOX_REAL_ENGINE2, else the newest download in $REAL_ENGINE2_CACHE.
# zoo needs SANDBOX=<Facepunch/sandbox checkout>.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
SCN=${1:?usage: tools/diff/run.sh <orbmatch|sweeper|zoo> [both|ours|real] [tol]}
WHICH=${2:-both}
TOL=${3:-0.5}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
OUT=$ROOT/target/diff/$SCN
mkdir -p "$OUT"
ART=${UNDERBOX_REAL_ENGINE2:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')}

one() {
    eng=$1
    real=
    [ "$eng" = real ] && { [ -n "$ART" ] || { echo "run: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; real=$ART; }
    echo "== $SCN on $eng"
    case $SCN in
        orbmatch)
            UNDERBOX_REAL_ENGINE2=$real sh "$ROOT/tools/match.sh" 120 > "$OUT/$eng.txt" 2>&1 || true
            cp "$ROOT/target/match/server.log" "$OUT/$eng.log" ;;
        probe)
            UNDERBOX_REAL_ENGINE2=$real OUT="$OUT/$eng" UNTIL='DIFF:END' GRACE=1 sh "$ROOT/tools/boot-server.sh" 120 "$SBOX" -- \
                +game "$ROOT/tools/diff/probe/.sbproj" > "$OUT/$eng.txt" 2>&1 || true
            cp "$OUT/$eng/server.log" "$OUT/$eng.log"
            grep -q 'DIFF:END' "$OUT/$eng.log" && echo "RESULT: PASS" >> "$OUT/$eng.txt" || echo "RESULT: FAIL" >> "$OUT/$eng.txt" ;;
        sweeper|zoo)
            dir=$SBOX/game/samples/sweeper
            [ "$SCN" = zoo ] && dir=${SANDBOX:?zoo needs SANDBOX=<Facepunch/sandbox checkout>}
            UNDERBOX_REAL_ENGINE2=$real OUT="$OUT/$eng" SECS=${SECS:-180} \
                sh "$ROOT/tools/game.sh" "$dir" > "$OUT/$eng.txt" 2>&1 || true
            cp "$OUT/$eng/server.log" "$OUT/$eng.log" ;;
        *) echo "run: unknown scenario '$SCN'" >&2; exit 2 ;;
    esac
    grep -E '^(PASS|FAIL|RESULT)' "$OUT/$eng.txt" | tail -n 8 || true
}

case $WHICH in
    both) one ours; one real ;;
    ours|real) one "$WHICH" ;;
    *) echo "run: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
[ -s "$OUT/ours.log" ] && [ -s "$OUT/real.log" ] || exit 0
python3 -I "$ROOT/tools/diff/compare.py" "$OUT/ours.log" "$OUT/real.log" 'MATCH:|GAME:|DIFF:' "$TOL" | tee "$OUT/report.txt"
