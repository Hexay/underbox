#!/bin/sh
# Boot Facepunch's game client (unmodified sbox-public, engine/Launcher/Sbox) on our libengine2,
# headless on SDL's offscreen driver, logging every presented frame's hash, and summarise.
# usage: [UNTIL=regex] [STOP_FRAME=n] [GRACE=s] [FRAMES=log|dir] tools/boot-client.sh [max-s=90] [sbox-public] [-- client args]
# Client args default to `-mcp +game corpus/orbmatch/.sbproj`. Stops GRACE (default 2) s after a
# log line matches UNTIL (default: presented frame STOP_FRAME, 300), else at max-s; with SETTLE=n, not
# before the last n presented frames also share a hash (tools/settle-watch.py). FRAMES=<dir>
# also dumps PPMs (UNDERBOX_FRAMES_EVERY=n keeps every n-th). Runs in this worktree's game dir (tools/game-dir.sh);
# SEED=<dir> overlays that dir's files for one boot.
# Logs: $OUT/client.log (managed's logs/dotnet.log), $OUT/client.out (stdout/stderr), $OUT/frames.tsv
# (frame, hash); OUT defaults to target/client. SHADOW_QUALITY=0..3 pins managed's stored shadow
# quality (Low..Ultra, tools/video-setting.py).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
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
# see tools/boot-server.sh (truncate first)
for f in client.out client.log frames.tsv; do : > "$OUT/$f"; done
t0=$(date +%s.%N)
lap() { printf '%s %.1fs' "$1" "$(echo "$(date +%s.%N) - $t0" | bc)"; }

sh "$ROOT/tools/submodules.sh"
GAME=$(UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" client)
# SEED=<dir>: files laid out like the game dir, copied in for this boot and removed after it.
SEEDED=
if [ -n "${SEED:-}" ]; then
    SEEDED=$(cd "$SEED" && find . -type f)
    cp -a "$SEED/." "$GAME/"
fi
trap 'rm -f "$GAME/.underbox-lease.$$"; for f in $SEEDED; do rm -f "$GAME/$f"; done' EXIT
T_PREP=$(lap prepare); t0=$(date +%s.%N)
sh "$ROOT/tools/install-engine2.sh" "$GAME"
T_CARGO=$(lap cargo); t0=$(date +%s.%N)
[ -z "${SHADOW_QUALITY:-}" ] || python3 -I "$ROOT/tools/video-setting.py" "$GAME" shadow.quality "$SHADOW_QUALITY"

# NLog names the file after the process: dotnet.
LOG=$GAME/logs/dotnet.log
rm -f "$LOG"
# For watchers that act on the live log (tools/game/play.sh); client.log is only copied at exit.
echo "$LOG" > "$OUT/client.logpath"
# No sound card here: a null default PCM, or libasound logs eight lines per device probe. Audio goes
# to engine2's null device, which drains in real time: ALSA's null PCM takes samples as fast as they
# come, so cpal's callback had managed mix about 15x faster than real time, burning the client's CPU.
printf 'pcm.!default { type null }\n' > "$OUT/asound.conf"
export UNDERBOX_AUDIO=${UNDERBOX_AUDIO:-null}
cd "$GAME"
# No display here: SDL falls back to its offscreen driver; force it so a stray DISPLAY can't win.
SDL_VIDEO_DRIVER=${SDL_VIDEO_DRIVER:-offscreen} UNDERBOX_FRAMES=${FRAMES:-log} ALSA_CONFIG_PATH="$OUT/asound.conf" \
    timeout -s TERM -k 10 "$SECS" dotnet sbox.dll "$@" > "$OUT/client.out" 2>&1 &
CLIENT=$!
if [ -n "$UNTIL" ]; then
    # The kill sits inside the pipeline's last stage: the pipeline itself only ends when tail does.
    stop_when() {
        if [ -n "${SETTLE:-}" ]; then python3 -I "$ROOT/tools/settle-watch.py" "$UNTIL" "$GRACE" "$SETTLE"
        else grep -qE -m1 -- "$UNTIL" && sleep "$GRACE"; fi
    }
    tail -n +1 -F --pid=$CLIENT "$LOG" "$OUT/client.out" 2>/dev/null |
        { stop_when && kill -TERM $CLIENT 2>/dev/null; } &
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
