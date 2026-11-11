#!/bin/sh
# Boot Facepunch's real engine2 client on a corpus scene under Xvfb + lavapipe and grab the window.
# usage: tools/diff/real-client.sh <corpus> <out-dir> [max-s=240] [artifact-root]
# Waits for `game.load success`, then GRACE (default 20) s, then grabs Xvfb's framebuffer every
# second until SETTLE (default 4) grabs in a row are identical; writes <out-dir>/frame.xwd plus
# client.out and client.log. See docs/differential-render.md.
# Real refuses lavapipe unless given its own -vulkan_allow_cpu (CPU device types are skipped), and
# waits forever for a Steam client unless given -nosteam.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
CORPUS=${1:?usage: tools/diff/real-client.sh <corpus> <out-dir> [max-s] [artifact-root]}
OUT=$2
SECS=${3:-240}
ART=${4:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')}
[ -n "$ART" ] || { echo "real-client: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }
GRACE=${GRACE:-20}
SETTLE=${SETTLE:-4}
SIZE=${SIZE:-1024x768}
PROJ=$ROOT/corpus/$CORPUS/.sbproj
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
rm -rf "$OUT"; mkdir -p "$OUT/fb"
GAME=$(UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" client)
LEASED=$GAME
GAME=$(sh "$ROOT/tools/diff/real-game.sh" "$GAME" "$ART" $$ +game "$PROJ")
# As tools/client-frame.sh pins ours.
python3 -I "$ROOT/tools/video-setting.py" "$GAME" shadow.quality "${SHADOW_QUALITY:-2}"
python3 -I "$ROOT/tools/video-setting.py" "$GAME" volumetricfog.quality "${FOG_QUALITY:-2}"
# The window sits at the root's origin only when the screen is the window's size (no WM).
D=:$(( 200 + $$ % 700 ))
Xvfb "$D" -screen 0 "${SIZE}x24" -fbdir "$OUT/fb" -nocursor -nolisten tcp > "$OUT/xvfb.log" 2>&1 &
XVFB=$!
CLIENT=
trap 'rm -f "$LEASED/.underbox-lease.$$"; [ -z "$CLIENT" ] || kill -TERM $CLIENT 2>/dev/null; kill $XVFB 2>/dev/null' EXIT
printf 'pcm.!default { type null }\n' > "$OUT/asound.conf"
LOG=$GAME/logs/dotnet.log
rm -f "$LOG"
(cd "$GAME" && DISPLAY=$D SDL_VIDEO_DRIVER=x11 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
    ALSA_CONFIG_PATH="$OUT/asound.conf" timeout -s TERM -k 10 "$SECS" \
    dotnet sbox.dll -nosteam -novr -vulkan_allow_cpu -mcp +game "$PROJ" > "$OUT/client.out" 2>&1) &
CLIENT=$!

grab() {
    until grep -q 'game.load success' "$LOG" 2>/dev/null; do
        kill -0 $CLIENT 2>/dev/null || return 1
        sleep 1
    done
    sleep "$GRACE"
    last= run=0 n=0
    while kill -0 $CLIENT 2>/dev/null; do
        cp "$OUT/fb/Xvfb_screen0" "$OUT/grab.xwd"
        sum=$(cksum < "$OUT/grab.xwd")
        if [ "$sum" = "$last" ]; then run=$((run + 1)); else run=1; last=$sum; fi
        [ "$run" -ge "$SETTLE" ] && { mv "$OUT/grab.xwd" "$OUT/frame.xwd"; return 0; }
        n=$((n + 1))
        [ "$n" -ge "${GRABS:-40}" ] && { mv "$OUT/grab.xwd" "$OUT/frame.xwd"; echo "real-client: unsettled after $n grabs; kept the last"; return 0; }
        sleep 1
    done
    return 1
}
STATUS=0
grab || STATUS=1
kill -TERM $CLIENT 2>/dev/null || true
wait $CLIENT 2>/dev/null || true
CLIENT=
cp "$LOG" "$OUT/client.log" 2>/dev/null || : > "$OUT/client.log"
grep -m1 -E 'Vulkan Physical Device' "$OUT/client.log" || true
grep -m1 -E 'game.load (success|fail)' "$OUT/client.log" | cut -c1-160 || echo "real-client: game never loaded"
[ "$STATUS" = 0 ] || { echo "real-client: no settled frame (see $OUT/client.log)"; exit 1; }
echo "real-client: $OUT/frame.xwd"
