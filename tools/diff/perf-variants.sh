#!/bin/sh
# Interleaved single runs of `perf.sh <scene> ours` under several environments, round by round, then
# one table of medians per variant against the first (docs/perf-bandwidth.md).
# usage: tools/diff/perf-variants.sh <scene> <label>=[VAR=value;VAR=value...] ...
#   tools/diff/perf-variants.sh showroom base= cmaa2='UNDERBOX_SKIP_LAYERS=cmaa2_cs'
#   tools/diff/perf-variants.sh skinned old='UNDERBOX_ENGINE2=/x/old.so' new='UNDERBOX_ENGINE2=/x/new.so'
# Env: ROUNDS=3, plus perf.sh's (WARMUP_FRAMES, RECORD, ...). Out: target/diff/perf-variants/<scene>/
# <label>-<round>/ (perf.sh's run dir), table.txt.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
SCENE=${1:?usage: tools/diff/perf-variants.sh <scene> <label>=[VAR=value ...] ...}
shift
ROUNDS=${ROUNDS:-3}
OUT=$ROOT/target/diff/perf-variants/$SCENE
rm -rf "$OUT"; mkdir -p "$OUT"
r=1
while [ "$r" -le "$ROUNDS" ]; do
    for v in "$@"; do
        label=${v%%=*} assigns=${v#*=}
        echo "== round $r: $label ($assigns)"
        # Split on ';' only: layer names hold spaces.
        old=$IFS; IFS=';'; set -f
        # shellcheck disable=SC2086
        env $assigns REPEATS=1 sh "$ROOT/tools/diff/perf.sh" "$SCENE" ours > "$OUT/$label-$r.log" 2>&1 || echo "perf-variants: $label round $r failed (see $OUT/$label-$r.log)"
        IFS=$old; set +f
        run=$(ls -d "$ROOT"/target/diff/perf/"$SCENE"*/ours-1 2>/dev/null | head -n 1)
        [ -n "$run" ] && mv "$run" "$OUT/$label-$r"
    done
    r=$((r + 1))
done
python3 -I "$ROOT/tools/diff/perf_variants.py" "$OUT" "$@" | tee "$OUT/table.txt"
