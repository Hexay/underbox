#!/bin/sh
# M1 exit test: host corpus/orbmatch headless and assert the scripted match's MATCH: markers.
# usage: tools/match.sh [seconds=90] [sbox-public]
# Log: target/match/server.log. Exit 0 only when every marker is present.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-90}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
OUT=$ROOT/target/match
LOG=$OUT/server.log
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" 0)}
mkdir -p "$OUT"

OUT=$OUT UNTIL='MATCH:END' GRACE=1 sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- \
    +game "$ROOT/corpus/orbmatch/.sbproj" +port "$PORT" > "$OUT/boot.txt" 2>&1 || true

fail=0
check() {
    if grep -qE "MATCH:$1" "$LOG"; then echo "PASS $2"; else echo "FAIL $2"; fail=1; fi
}
check 'START' 'match started'
check 'SPAWNED [1-9]' 'bots spawned'
check 'MOVE ' 'bots moved'
check 'COLLIDE ' 'bots collided'
check 'SCORE ' 'a bot scored'
check 'END winner=' 'match ended with a winner'

echo "== markers"; grep -oE 'MATCH:.*' "$LOG" | cut -c1-120 | sort | uniq -c | sort -rn | head -n 12 || true
echo "== errors"; grep -iE 'exception|error' "$LOG" | grep -v 'stub:' | cut -c1-200 | sort -u | head -n 8 || true
echo "== stubs hit: $(grep -oE 'stub: [A-Za-z0-9_]+' "$LOG" | sort -u | wc -l)"
if [ $fail -eq 0 ]; then
    echo "RESULT: PASS"
else
    # Keep the evidence: the next run overwrites server.log, and failures have been intermittent.
    keep=$OUT/fail-$(date +%Y%m%d-%H%M%S)
    mkdir -p "$keep" && cp "$LOG" "$OUT/boot.txt" "$keep/" 2>/dev/null || true
    echo "RESULT: FAIL (logs kept in $keep)"
fi
exit $fail
