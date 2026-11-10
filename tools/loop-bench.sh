#!/bin/sh
# Time an edit -> command loop: append a unique comment to FILE (defeats sccache), run CMD, restore.
# usage: [EDIT=append|prepend] tools/loop-bench.sh <runs> <file> <cmd...>
# e.g.   tools/loop-bench.sh 5 crates/engine2/src/math.rs cargo test -q -p engine2 -j4
# Prints each run's wall seconds, then median and min..max. FILE is restored on exit.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
RUNS=$1; FILE=$ROOT/$2; shift 2
BAK=$(mktemp); cp "$FILE" "$BAK"
trap 'cp "$BAK" "$FILE"; rm -f "$BAK"' EXIT INT TERM
TIMES=""
i=0
while [ $i -lt "$RUNS" ]; do
    i=$((i + 1))
    # EDIT=prepend shifts every span in FILE: the harder case for incremental compilation.
    { [ "${EDIT:-append}" = prepend ] && echo "// loop-bench $$ $i $(date +%N)"; cat "$BAK"
      [ "${EDIT:-append}" = append ] && echo "// loop-bench $$ $i $(date +%N)"; } > "$FILE" || true
    t0=$(date +%s.%N)
    ( cd "$ROOT" && "$@" ) > /dev/null 2>&1 || echo "run $i: command failed" >&2
    t=$(echo "$(date +%s.%N) - $t0" | bc)
    printf 'run %d: %.1fs\n' "$i" "$t"
    TIMES="$TIMES $t"
done
echo "$TIMES" | tr ' ' '\n' | sed '/^$/d' | sort -n | awk '{a[NR]=$1} END {printf "median %.1fs  (min %.1f, max %.1f, n=%d)\n", a[int((NR+1)/2)], a[1], a[NR], NR}'
