#!/bin/sh
# Play corpus/sound on our engine2 and on Facepunch's, capture each one's final mix, compare.
# usage: tools/diff/sound-run.sh [both|ours|real] [artifact-root]   (or tools/diff/run.sh sound ...)
# Both engines write their output through SDL's disk audio driver contract (SDL_AUDIO_DRIVER=disk,
# SDL_AUDIO_DISK_OUTPUT_FILE): real through its own SDL3, ours through engine2's disk device.
# Out: target/diff/sound/{ours,real}/{audio.raw,client.log}, report.txt (tools/diff/sound.py).
# See docs/differential-audio.md.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
WHICH=${1:-both}
ART=${2:-}
OUT=$ROOT/target/diff/sound
PROJ=$ROOT/corpus/sound/.sbproj
mkdir -p "$OUT"

one() {
    eng=$1
    echo "== sound on $eng"
    rm -rf "$OUT/$eng"; mkdir -p "$OUT/$eng"
    if [ "$eng" = real ]; then
        [ -n "$ART" ] || ART=$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')
        [ -n "$ART" ] || { echo "sound: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }
        set -- env UNDERBOX_REAL_ENGINE2="$ART" UNDERBOX_STANDIN_FRIENDS=1
    else
        set -- env UNDERBOX_AUDIO=disk
    fi
    "$@" SDL_AUDIO_DRIVER=disk SDL_AUDIO_DISK_OUTPUT_FILE="$OUT/$eng/audio.raw" OUT="$OUT/$eng" \
        UNTIL='SOUND:END' GRACE=1 sh "$ROOT/tools/boot-client.sh" "${SECS:-240}" "" -- -mcp +game "$PROJ" \
        > "$OUT/$eng.txt" 2>&1 || true
    grep -hE 'SOUND:' "$OUT/$eng/client.log" | sed 's/^.*SOUND:/  SOUND:/' | cut -c1-120 || true
}

case $WHICH in
    both) one ours; one real ;;
    ours|real) one "$WHICH" ;;
    *) echo "sound: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
A=$OUT/ours/audio.raw
B=$OUT/real/audio.raw
[ -s "$A" ] && [ -s "$B" ] || { echo "RESULT: FAIL (missing capture: $A or $B)"; exit 1; }
python3 -I "$ROOT/tools/diff/sound.py" "$A" "$B" | tee "$OUT/report.txt"
