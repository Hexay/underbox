#!/bin/sh
# Run a client command in an X window on the virtual display xvfb-run gave us, and drive it with XTest
# (tools/game/xinput.py) once the client's live log matches PLAY_TRIGGER.
# usage: xvfb-run -a -s '-screen 0 WxHx24' sh tools/game/play.sh <out> <client command...>
# PLAY_STEPS: xinput.py steps (default: focus the window, hold W for 6 s, then turn right). Long, since a
# lavapipe client runs at a few frames a second.
# Writes $OUT/xinput.log; the client command is expected to write $OUT/client.logpath (boot-client.sh).
set -eu
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
OUT=$1
shift
: "${DISPLAY:?play.sh runs under xvfb-run}"
TRIGGER=${PLAY_TRIGGER:-GAME:SETTLED}
STEPS=${PLAY_STEPS:-center focus wait:1 hold:w:6 wait:1 move:600:0:4 wait:1}
WAIT=${PLAY_WAIT:-900}
rm -f "$OUT/client.logpath"
: > "$OUT/xinput.log"

drive() {
    i=0
    until [ -s "$OUT/client.logpath" ] && grep -qE -- "$TRIGGER" "$(cat "$OUT/client.logpath")" 2>/dev/null; do
        i=$((i + 1)); [ $i -lt "$WAIT" ] || { echo "xinput: no /$TRIGGER/ in ${WAIT} s" >> "$OUT/xinput.log"; return; }
        sleep 1
    done
    echo "xinput: trigger /$TRIGGER/ seen on $DISPLAY" >> "$OUT/xinput.log"
    # shellcheck disable=SC2086
    python3 -I "$HERE/xinput.py" "$DISPLAY" $STEPS >> "$OUT/xinput.log" 2>&1 || echo "xinput: failed ($?)" >> "$OUT/xinput.log"
}

drive &
DRIVER=$!
SDL_VIDEO_DRIVER=x11 "$@" || true
kill "$DRIVER" 2>/dev/null || true
