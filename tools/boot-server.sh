#!/bin/sh
# Boot Facepunch's dedicated server (unmodified sbox-public) on our libengine2 and summarise.
# usage: [UNTIL=regex] [GRACE=s] tools/boot-server.sh [max-seconds=60] [sbox-public] [-- extra server args]
# Stops GRACE (default 2) seconds after a log line matches UNTIL, else at max-seconds. UNTIL defaults
# to 'Server is idle' for a plain boot and to none (run to max-seconds) when server args are given.
# Log: $OUT/server.log (default target/boot). Each stub slot logs once, on its first call.
# Runs in this worktree's private game dir (tools/game-dir.sh); no lock needed around it.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-60}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
shift $(( $# > 2 ? 2 : $# ))
[ "${1:-}" = "--" ] && shift
[ $# -eq 0 ] && UNTIL=${UNTIL-Server is idle}
UNTIL=${UNTIL:-}
GRACE=${GRACE:-2}
# Close an inherited boot-lock fd (from a raw `flock <lock> ...` caller) so lingering children can't hold it.
for fd in /proc/$$/fd/[3-9]; do
    case $(readlink "$fd" 2>/dev/null) in *underbox-boot.lock) eval "exec ${fd##*/}>&-" ;; esac
done
OUT=${OUT:-$ROOT/target/boot}
mkdir -p "$OUT"
t0=$(date +%s.%N)
lap() { printf '%s %.1fs' "$1" "$(echo "$(date +%s.%N) - $t0" | bc)"; }

sh "$ROOT/tools/submodules.sh"
GAME=$(sh "$ROOT/tools/game-dir.sh" "$SBOX")
T_PREP=$(lap prepare); t0=$(date +%s.%N)

# see docs/dev-loop.md (incremental)
env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4
T_CARGO=$(lap cargo); t0=$(date +%s.%N)
NATIVE="$GAME/bin/linuxsteamrt64"
mkdir -p "$NATIVE"
# Rename, not overwrite: a server still mapping the old file would crash on an in-place rewrite.
cp --reflink=auto "${CARGO_TARGET_DIR:-$ROOT/target}/debug/libengine2.so" "$NATIVE/.libengine2.so.$$"
mv -f "$NATIVE/.libengine2.so.$$" "$NATIVE/libengine2.so"
# Managed loads these modules by name for their CreateInterface; they're all engine2, and the
# loader dedupes symlinks by inode so they share its state. TODO(windows): needs forwarder DLLs.
for m in vfx_vulkan filesystem_stdio; do ln -sf libengine2.so "$NATIVE/lib$m.so"; done
# Facepunch ships only SkiaSharp's Windows/macOS natives; match Sandbox.Engine.csproj's version.
# s&box's DLLImportResolver loads natives from its native dir, not runtimes/.
sh "$ROOT/tools/nuget-file.sh" SkiaSharp.NativeAssets.Linux 3.116.0 runtimes/linux-x64/native/libSkiaSharp.so \
    "$NATIVE/libSkiaSharp.so"

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
