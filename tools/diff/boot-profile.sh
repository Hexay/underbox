#!/bin/sh
# One `run.sh perf <scenario>` boot under `perf record`, reported by boot phase (docs/perf-boot.md).
# usage: tools/diff/boot-profile.sh <scenario> [label]
# Env as perf.sh (UNDERBOX_ENGINE2=<so> recommended), plus ENGINE=ours|real, BOOT_PROFILE_CALLGRAPH=1.
# Out: target/diff/boot-profile/<scenario>-<label>/{perf.data,report.txt,managed.log}
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
SCN=${1:?usage: tools/diff/boot-profile.sh <scenario> [label]}
LABEL=${2:-run}
ENGINE=${ENGINE:-ours}
case $SCN in showroom|skinned) SIDE=${SIDE:-client} ;; *) SIDE=${SIDE:-server} ;; esac
RUN=$ROOT/target/diff/perf/$SCN; [ "$SIDE" = server ] || RUN=$RUN-$SIDE
PROF=$ROOT/target/diff/boot-profile/$SCN-$LABEL
rm -rf "$PROF"; mkdir -p "$PROF"
if [ "$SIDE" = server ]; then LOG=$RUN/$ENGINE-1/server.log; STDOUT=$LOG; else LOG=$RUN/$ENGINE-1/client.log; STDOUT=$RUN/$ENGINE-1/client.out; fi
# Own session: the server's teardown signals its process group, which would kill perf before it writes.
setsid python3 -I "$ROOT/tools/diff/boot_profile.py" attach "$STDOUT" "$PROF" &
ATTACH=$!
# Double-mapped JIT code (W^X) hides managed symbols from perf's map.
DOTNET_PerfMapEnabled=1 DOTNET_EnableWriteXorExecute=0 REPEATS=1 SIDE=$SIDE \
    sh "$ROOT/tools/diff/perf.sh" "$SCN" "$ENGINE" > "$PROF/perf-run.txt" 2>&1 || true
wait "$ATTACH" || true
cp "$LOG" "$PROF/managed.log" 2>/dev/null || true
grep -E '^boot ' "$RUN/report.txt" > "$PROF/perf-summary.txt" 2>/dev/null || true
python3 -I "$ROOT/tools/diff/boot_profile.py" report "$PROF" "$PROF/managed.log" > "$PROF/report.txt"
cat "$PROF/perf-summary.txt"
