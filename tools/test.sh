#!/bin/sh
# Run one crate's tests and print only what matters: compiler errors, failures, the totals.
# usage: tools/test.sh <crate> [test-name-filter] [-- test-binary args, e.g. --ignored]
# Exits with cargo's status. Use this instead of `cargo test | grep`, which hides a failing exit.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
LOG=$(mktemp); trap 'rm -f "$LOG"' EXIT
sh "$ROOT/tools/submodules.sh"
CRATE=$1; FILTER=${2:-}; shift; [ $# -gt 0 ] && shift; [ "${1:-}" = "--" ] && shift
status=0
# see docs/dev-loop.md (incremental)
env -u CARGO_INCREMENTAL cargo test -q --manifest-path "$ROOT/Cargo.toml" -p "$CRATE" -j4 -- ${FILTER:+"$FILTER"} "$@" > "$LOG" 2>&1 || status=$?
grep -E '^(error|warning: unused)|^---- |panicked at|^test .* FAILED|^failures:' "$LOG" | head -n 40 || true
awk '/^test result:/ { p += $4; f += $6; i += $8 } END { printf "== %d passed, %d failed, %d ignored\n", p, f, i }' "$LOG"
[ $status -eq 0 ] && echo "RESULT: PASS" || { tail -n 15 "$LOG"; echo "RESULT: FAIL (exit $status)"; }
exit $status
