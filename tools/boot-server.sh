#!/bin/sh
# Boot Facepunch's dedicated server (unmodified sbox-public) on our libengine2 and summarise.
# usage: [UNTIL=regex] [GRACE=s] tools/boot-server.sh [max-seconds=60] [sbox-public] [-- extra server args]
# Stops GRACE (default 2) seconds after a log line matches UNTIL, else at max-seconds. UNTIL defaults
# to 'Server is idle' for a plain boot and to none (run to max-seconds) when server args are given.
# Log: $OUT/server.log (default target/boot). Each stub slot logs once, on its first call.
# Runs in a game dir copy no other boot touches (tools/game-dir.sh); no lock needed around it.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-60}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
shift $(( $# > 2 ? 2 : $# ))
[ "${1:-}" = "--" ] && shift
[ $# -eq 0 ] && UNTIL=${UNTIL-Server is idle}
UNTIL=${UNTIL:-}
GRACE=${GRACE:-2}
case " $* " in *" +port "*) ;; *) set -- "$@" +port "$(sh "$ROOT/tools/worktree-port.sh" $$)" ;; esac
# Close an inherited boot-lock fd (from a raw `flock <lock> ...` caller) so lingering children can't hold it.
for fd in /proc/$$/fd/[3-9]; do
    case $(readlink "$fd" 2>/dev/null) in *underbox-boot.lock) eval "exec ${fd##*/}>&-" ;; esac
done
OUT=${OUT:-$ROOT/target/boot}
mkdir -p "$OUT"
# Before anything can fail: callers grep this log, and a stale one passes their checks. Also before
# the fork: the watcher's tail can open the log before the job's redirect truncates it.
: > "$OUT/server.log"
t0=$(date +%s.%N)
lap() { printf '%s %.1fs' "$1" "$(echo "$(date +%s.%N) - $t0" | bc)"; }

sh "$ROOT/tools/submodules.sh"
GAME=$(UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX")
trap 'rm -f "$GAME/.underbox-lease.$$"' EXIT
T_PREP=$(lap prepare); t0=$(date +%s.%N)

sh "$ROOT/tools/install-engine2.sh" "$GAME"
T_CARGO=$(lap cargo); t0=$(date +%s.%N)

cd "$GAME"
# TERM, not INT: a `&` background sh job inherits SIGINT ignored, and .NET then never hooks it.
timeout -s TERM -k 10 "$SECS" dotnet sbox-server.dll "$@" > "$OUT/server.log" 2>&1 &
SERVER=$!
if [ -n "$UNTIL" ]; then
    # The kill sits inside the pipeline's last stage: the pipeline itself only ends when tail does.
    tail -n +1 -F --pid=$SERVER "$OUT/server.log" 2>/dev/null |
        { grep -qE -m1 -- "$UNTIL" && sleep "$GRACE" && kill -TERM $SERVER 2>/dev/null; } &
fi
wait $SERVER || echo "exit: $?"
T_SERVER=$(lap server)

echo "== engine2"; grep -E '^engine2:' "$OUT/server.log" | head -n 5 || true
echo "== first stubs called"; grep -E '^\[Warn\] stub:' "$OUT/server.log" | head -n 15 || true
echo "== stub slots hit: $(grep -cE '^\[Warn\] stub:' "$OUT/server.log" || true)"
echo "== errors"; grep -iE 'exception|error|fatal' "$OUT/server.log" | grep -v '^\[Warn\] stub' | head -n 15 || true
echo "== tail"; tail -n 8 "$OUT/server.log"
echo "== timing: $T_PREP, $T_CARGO, $T_SERVER${UNTIL:+ (until /$UNTIL/)}"
