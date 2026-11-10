#!/bin/sh
# Boot Facepunch's game client (unmodified sbox-public, engine/Launcher/Sbox) on our libengine2,
# headless on SDL's offscreen driver, logging every presented frame's hash, and summarise.
# usage: [UNTIL=regex] [STOP_FRAME=n] [GRACE=s] [FRAMES=log|dir] tools/boot-client.sh [max-s=90] [sbox-public] [-- client args]
# Client args default to `-mcp +game corpus/orbmatch/.sbproj`. Stops GRACE (default 2) s after a
# log line matches UNTIL (default: presented frame STOP_FRAME, 300), else at max-s. FRAMES=<dir>
# also dumps PPMs (UNDERBOX_FRAMES_EVERY=n keeps every n-th). Takes the boot lock itself.
# Logs: $OUT/client.log (managed's logs/dotnet.log), $OUT/client.out (stdout/stderr), $OUT/frames.tsv
# (frame, hash); OUT defaults to target/client.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
LOCK=/srv/agents/cache/underbox-boot.lock
if [ -z "${UNDERBOX_BOOT_LOCKED:-}" ]; then
    # -o: a lingering child (MSBuild node, the client) must not inherit and keep holding the lock.
    UNDERBOX_BOOT_LOCKED=1 exec flock -o "$LOCK" sh "$0" "$@"
fi
SECS=${1:-90}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
shift $(( $# > 2 ? 2 : $# ))
[ "${1:-}" = "--" ] && shift
# -mcp: a client only loads a game from a .sbproj path with it (GameInstanceDll.StartGame).
[ $# -eq 0 ] && set -- -mcp +game "$ROOT/corpus/orbmatch/.sbproj"
UNTIL=${UNTIL-frame: [^ ]+ #${STOP_FRAME:-300} }
GRACE=${GRACE:-2}
OUT=${OUT:-$ROOT/target/client}
mkdir -p "$OUT"
t0=$(date +%s.%N)
lap() { printf '%s %.1fs' "$1" "$(echo "$(date +%s.%N) - $t0" | bc)"; }

sh "$ROOT/tools/prepare-sbox.sh" "$SBOX"
# A stamp, not the dll's mtime (see boot-server.sh).
STAMP="$SBOX/game/.underbox-client-built"
if [ ! -f "$SBOX/game/sbox.dll" ] || [ "$SBOX/engine/Sandbox.Compiling/EmbeddedRefs" -nt "$STAMP" ]; then
    echo "== building sbox client (~2 min)"
    # The icon isn't in the public repo; an empty property skips it without patching their csproj.
    dotnet build "$SBOX/engine/Launcher/Sbox/Sbox.csproj" -c Release -m:4 -nodeReuse:false -v q -nologo -clp:ErrorsOnly -p:ApplicationIcon=
    touch "$STAMP"
fi
sh "$ROOT/tools/r2r-managed.sh" "$SBOX"
T_PREP=$(lap prepare); t0=$(date +%s.%N)
sh "$ROOT/tools/install-engine2.sh" "$SBOX"
T_CARGO=$(lap cargo); t0=$(date +%s.%N)

# NLog names the file after the process: dotnet.
LOG=$SBOX/game/logs/dotnet.log
rm -f "$LOG"
# No sound card here: a null default PCM, or libasound logs eight lines per device probe.
printf 'pcm.!default { type null }\n' > "$OUT/asound.conf"
cd "$SBOX/game"
# No display here: SDL falls back to its offscreen driver; force it so a stray DISPLAY can't win.
SDL_VIDEO_DRIVER=${SDL_VIDEO_DRIVER:-offscreen} UNDERBOX_FRAMES=${FRAMES:-log} ALSA_CONFIG_PATH="$OUT/asound.conf" \
    timeout -s TERM -k 10 "$SECS" dotnet sbox.dll "$@" > "$OUT/client.out" 2>&1 &
CLIENT=$!
if [ -n "$UNTIL" ]; then
    # The kill sits inside the pipeline's last stage: the pipeline itself only ends when tail does.
    tail -n +1 -F --pid=$CLIENT "$LOG" "$OUT/client.out" 2>/dev/null |
        { grep -qE -m1 -- "$UNTIL" && sleep "$GRACE" && kill -TERM $CLIENT 2>/dev/null; } &
fi
wait $CLIENT || echo "exit: $?"
T_CLIENT=$(lap client)
cp "$LOG" "$OUT/client.log" 2>/dev/null || : > "$OUT/client.log"

all() { cat "$OUT/client.log" "$OUT/client.out"; }
all | grep -oE 'frame: [^ ]+ #[0-9]+ [0-9x]+ [0-9a-f]+' | awk '{print $3 "\t" $5}' > "$OUT/frames.tsv" || true
echo "== stub slots hit: $(all | grep -oE 'stub: [A-Za-z0-9_]+' | sort -u | wc -l)"
all | grep -oE 'stub: [A-Za-z0-9_]+' | awk '!seen[$0]++' | head -n 25 | sed 's/^/  /' || true
echo "== errors"; all | grep -iE 'exception|error|fatal|panick' | grep -v 'stub:' | cut -c1-220 | awk '!seen[$0]++' | head -n 20 || true
echo "== frames: $(wc -l < "$OUT/frames.tsv") presented; distinct hashes (count, first frame, hash):"
awk '{ n[$2]++; if (!($2 in first)) first[$2] = $1 } END { for (h in n) print n[h], first[h], h }' "$OUT/frames.tsv" |
    sort -t'#' -k2,2n | sed 's/^/  /' | head -n 12
echo "== tail"; tail -n 6 "$OUT/client.log" | cut -c1-200; tail -n 3 "$OUT/client.out" | cut -c1-200
echo "== timing: $T_PREP, $T_CARGO, $T_CLIENT${UNTIL:+ (until /$UNTIL/)}"
