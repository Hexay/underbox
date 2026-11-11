#!/bin/sh
# Boot a real game from a checkout (Facepunch/sandbox, apetavern/grubs, sbox-public's samples) on our
# libengine2; print evidence lines and a PASS/FAIL summary.
# usage: [MODE=server|join|interop|host|grubs|walker|render|play|sound] [MAP=<map>] [SECS=300] [SBOX=<sbox-public>] [COMPILED_ONLY=1] tools/game.sh <checkout> [scene]
#   MAP     passed as `+game <sbproj> <map>`: what a MapInstance with UseMapFromLaunch loads
#   server  dedicated server on the scene until its bodies settle and it has idled
#   join    the server plus a second instance joining it with +connect; PASS when the client's own player
#           stands on the floor. The client gets an offline package record for the local checkout
#           (tools/game/offline_package.py) and a loopback HTTP server answering that record's manifest URL
#   host    a headless client hosting the scene; START_CMD runs once it settles
#   grubs   host on scenes/beach.scene with START_CMD=gr_ffa_next_state (Grubs' "start the round");
#           PASS when terrain and equipment load
#   walker  join on sbox-walker's own map (MAP defaults to scenes/scenemap.scene); PASS adds the joined
#           player standing on the map's floor (docs/walker.md)
#   interop join with Facepunch's engine2 as the client (docs/interop.md); SKIP without its artifacts
#   render  host; its last frame goes to $OUT/frame.png
#   play    a client in a real SDL window on Xvfb, driven by XTest (tools/game/play.sh): hold W, then turn
#           the mouse. PLAY_VIA=join (default): PASS when the dedicated server sees the joined player move
#           and turn. PLAY_VIA=host: PASS when the host's camera moves and turns.
#   sound   host with audio stats (UNDERBOX_AUDIO_STATS); PASS when voices start and audible blocks reach the device
# Scene: scenes/beach.scene for grubs, else scenes/zoo.scene if the checkout has it, else the project's StartupScene.
# Clients render on lavapipe: headless (SDL offscreen) except in play. UNDERBOX_RENDER=empty fails a client
# at its swap chain. CLIENT_ARGS: extra client launch args (e.g. +hotload_log 2).
# The checkout stays read-only: runs happen in a scratch copy under $UNDERBOX_GAME_COPIES, which gets the
# watcher tools/game/RealGameWatch.cs and the chosen startup scene. Stops at the watcher's GAME:END.
# Optional and never part of tools/verify.sh: it needs checkouts this repo does not ship.
# Package downloads stay off: the game processes get a dead HTTP(S) proxy, so any request for sbox.game
# fails instead of fetching. Only the join client may reach 127.0.0.1, where the manifest server is.
# Logs: $OUT/server.log, $OUT/client.log, $OUT/client.out (OUT defaults to target/game/<checkout name>).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
CHECKOUT=$(cd -- "${1:?usage: tools/game.sh <checkout> [scene]}" && pwd)
MODE=${MODE:-server}
PLAY_VIA=${PLAY_VIA:-join}
if [ "$MODE" = walker ]; then MODE=join WALKER=1 MAP=${MAP-scenes/scenemap.scene}; fi
if [ "$MODE" = interop ]; then
    . "$ROOT/tools/diff/env.sh"
    CLIENT_ENGINE2=${CLIENT_ENGINE2:-${UNDERBOX_REAL_ENGINE2:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/addons/menu 2>/dev/null |
        head -n 1 | sed 's#/game/addons/menu$##')}}
    [ -n "$CLIENT_ENGINE2" ] || { echo "SKIP: no real engine2 artifacts with addons (docs/interop.md)"; exit 0; }
    MODE=join INTEROP=1 SERVER_ENGINE2=
fi
case $MODE in
    server) IDLE=15 ;;
    join) IDLE=90 ;;
    grubs) IDLE=40 START_CMD=${START_CMD-gr_ffa_next_state} ;;
    host|render|sound) IDLE=40 ;;
    # The input arrives late (after a join): the watcher must not end first.
    play) IDLE=900 ;;
    *) echo "game.sh: unknown MODE '$MODE'" >&2; exit 2 ;;
esac
if [ -n "${2:-}" ]; then SCENE=$2
elif [ "$MODE" = grubs ]; then SCENE=scenes/beach.scene
elif [ -f "$CHECKOUT/Assets/scenes/zoo.scene" ]; then SCENE=scenes/zoo.scene
else SCENE=; fi
SECS=${SECS:-300}
# Facepunch's engine2 artifact root per side, or empty for ours (docs/interop.md).
CLIENT_ENGINE2=${CLIENT_ENGINE2-${UNDERBOX_REAL_ENGINE2:-}}
SERVER_ENGINE2=${SERVER_ENGINE2-${UNDERBOX_REAL_ENGINE2:-}}
NAME=$(basename -- "$CHECKOUT")
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${SBOX:-}")
OUT=${OUT:-$ROOT/target/game/$NAME}
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" $$)}
. "$ROOT/tools/cache-dir.sh"
COPIES=$UNDERBOX_GAME_COPIES
COPY=$COPIES/$NAME-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
mkdir -p "$OUT" "$COPIES"
. "$ROOT/tools/game/report.sh"
SERVER_SIDE=0
case $MODE in server|join) SERVER_SIDE=1 ;; play) [ "$PLAY_VIA" = host ] || SERVER_SIDE=1 ;; esac

# Build first, with the network reachable for cargo/NuGet; the game itself then runs with it blocked.
sh "$ROOT/tools/submodules.sh"
[ "$SERVER_SIDE" = 0 ] || UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" > /dev/null
[ "$MODE" = server ] || UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" client > /dev/null
env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4

[ -d "$COPY" ] || cp -a --reflink=auto "$CHECKOUT" "$COPY"
rsync -a --delete --exclude=.git "$CHECKOUT/" "$COPY/"
# Assets/ and assets/ (Code/ and code/) are one folder where a game was authored (Windows), two here,
# and only the capitalised one is read.
for d in Assets Code; do
    lower=$(echo "$d" | tr 'A-Z' 'a-z')
    if [ -d "$COPY/$lower" ]; then
        mkdir -p "$COPY/$d"
        rsync -a --ignore-existing "$COPY/$lower/" "$COPY/$d/"
        rm -rf "${COPY:?}/$lower"
    fi
done
# COMPILED_ONLY=1: drop project-JSON sources that ship their _c, so the game reads the shipped
# compiled files rather than ours (engine2 recompiles project JSON resources at mount).
if [ "${COMPILED_ONLY:-0}" = 1 ]; then
    find "$COPY/Assets" -type f \( -name '*.geq' -o -name '*.sdflayer' -o -name '*.surface' -o -name '*.vsnap' \) \
        -exec sh -c 'for f; do [ -f "${f}_c" ] && rm -f "$f"; done' sh {} +
fi
PROJECT=$(python3 -I "$ROOT/tools/game/prepare.py" "$COPY" "$SCENE" "$ROOT/tools/game/RealGameWatch.cs" \
    --idle "$IDLE" --command "${START_CMD:-}")
SCENE=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8-sig"))["Metadata"]["StartupScene"])' "$PROJECT")

export HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9
export http_proxy=$HTTP_PROXY https_proxy=$HTTPS_PROXY all_proxy=$ALL_PROXY NO_PROXY= no_proxy=
export UNDERBOX_AUDIO_STATS=1
CLIENT_UNTIL='GAME:END'
if [ "$MODE" = play ]; then
    command -v xvfb-run > /dev/null || { echo "game.sh: MODE=play needs xvfb-run (apt install xvfb)" >&2; exit 2; }
    [ "$PLAY_VIA" = host ] && export PLAY_TRIGGER=${PLAY_TRIGGER:-GAME:SETTLED} || export PLAY_TRIGGER=${PLAY_TRIGGER:-GAME:PLAYER_STANDING .*local=True}
    # The grace covers the rest of the mouse move and the screenshot after it (tools/game/play.sh).
    CLIENT_UNTIL='GAME:CAMERA_TURNED|GAME:END|Kicked|Disconnected' CLIENT_GRACE=10
fi

# run_client <env assignments...> <command...>: in play, inside a window on a virtual display with XTest input.
run_client() {
    if [ "$MODE" = play ]; then
        xvfb-run -a -s "-screen 0 ${PLAY_SCREEN:-1280x720}x24" sh "$ROOT/tools/game/play.sh" "$OUT" env "$@"
    else
        env "$@"
    fi > "$OUT/client-boot.txt" 2>&1 || true
}

host_client() {
    # shellcheck disable=SC2086
    run_client UNDERBOX_REAL_ENGINE2="$CLIENT_ENGINE2" OUT="$OUT" FRAMES="$OUT/frames" UNDERBOX_FRAMES_LAST=1 UNTIL="$CLIENT_UNTIL" GRACE="${CLIENT_GRACE:-3}" SHADOW_QUALITY="${SHADOW_QUALITY:-2}" \
        sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$PROJECT" ${MAP:+"$MAP"} ${CLIENT_ARGS:-}
}

# The join client: offline package seed, the manifest server on a borrowed port, then +connect.
join_client() {
    hport=$(sh "$ROOT/tools/worktree-port.sh" $$)
    rm -rf "$OUT/seed" "$OUT/serve"
    python3 -I "$ROOT/tools/game/offline_package.py" "$COPY" "$OUT/seed" \
        --manifest-url "http://127.0.0.1:$hport/manifest.json" --manifest-out "$OUT/serve/manifest.json" > /dev/null
    python3 -I -m http.server --bind 127.0.0.1 --directory "$OUT/serve" "$hport" > "$OUT/http.log" 2>&1 &
    http=$!
    # A leaked server holds the port, and the next join's client then reads a stale cached manifest.
    trap 'kill "$http" 2>/dev/null || true' EXIT INT TERM
    until='GAME:PLAYER_STANDING .*local=True|wasn.t found|Unable to download|Kicked|Disconnected' grace=3
    [ "$MODE" = play ] && until=$CLIENT_UNTIL'|wasn.t found|Unable to download' grace=$CLIENT_GRACE
    # shellcheck disable=SC2086
    run_client UNDERBOX_REAL_ENGINE2="$CLIENT_ENGINE2" NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 SEED="$OUT/seed" OUT="$OUT" GRACE=$grace UNTIL="$until" \
        FRAMES="$OUT/frames" UNDERBOX_FRAMES_LAST=1 sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- +connect "127.0.0.1:$PORT" ${CLIENT_ARGS:-}
    kill "$http" 2>/dev/null || true
}

t0=$(date +%s)
rm -rf "$OUT/frames" "$OUT/frame.png" "$OUT/screen.ppm" "$OUT/screen.png"; mkdir -p "$OUT/frames"
if [ "$SERVER_SIDE" = 0 ]; then
    host_client
    LOG=$OUT/client.log
    echo "== $NAME ($MODE) $SCENE: $(( $(date +%s) - t0 )) s"
    report_game "$LOG" client
    grep -hE 'render device' "$OUT/client.out" | head -n 1 || true
else
    # Truncated here, not only by boot-server.sh: the join client must not read a previous run's marker.
    : > "$OUT/server.log"
    # join: the server stops once the joined player stands, with time for the client to see it too.
    case $MODE in
        join) until='GAME:END|GAME:PLAYER_STANDING' grace=20 ;;
        play) until='GAME:END|GAME:PLAYER_TURNED|\] disconnected' grace=12 ;;
        *) until='GAME:END' grace=3 ;;
    esac
    UNDERBOX_REAL_ENGINE2=$SERVER_ENGINE2 OUT=$OUT UNTIL=$until GRACE=$grace \
        sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- +game "$PROJECT" ${MAP:+"$MAP"} +port "$PORT" > "$OUT/boot.txt" 2>&1 &
    SERVER=$!
    if [ "$MODE" != server ]; then
        until grep -q 'GAME:SETTLED' "$OUT/server.log" 2>/dev/null; do
            kill -0 "$SERVER" 2>/dev/null || break
            sleep 1
        done
        kill -0 "$SERVER" 2>/dev/null && join_client
    fi
    wait "$SERVER" || true
    LOG=$OUT/server.log
    echo "== $NAME ($MODE) $SCENE: $(( $(date +%s) - t0 )) s, port $PORT"
    report_game "$LOG" server
    if [ "$MODE" != server ]; then
        report_game "$OUT/client.log" client
        echo "== manifest server requests"; grep -oE '"GET [^"]*" [0-9]+' "$OUT/http.log" || true
    fi
fi
[ "$MODE" = play ] && { echo "== input"; cat "$OUT/xinput.log"; }

echo "== summary"
. "$ROOT/tools/game/checks.sh"
[ -f "$OUT/frames/GameWindow.ppm" ] && python3 -I "$ROOT/tools/ppm2png.py" "$OUT/frames/GameWindow.ppm" "$OUT/frame.png" > /dev/null &&
    echo "== frame: $OUT/frame.png"
[ "$MODE" = play ] && [ -f "$OUT/screen.ppm" ] && python3 -I "$ROOT/tools/ppm2png.py" "$OUT/screen.ppm" "$OUT/screen.png" > /dev/null &&
    echo "== screen (as X shows it): $OUT/screen.png"
if [ "$FAILED" = 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL (logs: $OUT)"; exit 1; fi
