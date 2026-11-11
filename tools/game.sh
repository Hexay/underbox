#!/bin/sh
# Boot a real game from a checkout (Facepunch/sandbox, apetavern/grubs) on our libengine2; print evidence
# lines and a PASS/FAIL summary.
# usage: [MODE=server|join|grubs|render] [SECS=300] [SBOX=<sbox-public>] [COMPILED_ONLY=1] tools/game.sh <checkout> [scene]
#   server  dedicated server on the scene (default scenes/zoo.scene) until its bodies settle and it has idled
#   join    the server plus a second instance joining it with +connect; PASS when the client's own player
#           stands on the floor. The client gets an offline package record for the local checkout
#           (tools/game/offline_package.py) and a loopback HTTP server answering that record's manifest URL
#   grubs   a headless client hosting the scene (default scenes/beach.scene) that runs START_CMD (default
#           gr_ffa_next_state, Grubs' "start the round") once it settles; PASS when terrain and equipment load
#   render  a headless client hosting the scene; its last frame goes to $OUT/frame.png
# Clients render headless on lavapipe: UNDERBOX_RENDER=empty fails a client at its swap chain.
# CLIENT_ARGS: extra client launch args (e.g. +hotload_log 2).
# The checkout stays read-only: runs happen in a scratch copy under /srv/agents/cache, which gets the
# watcher tools/game/RealGameWatch.cs and the chosen startup scene. Stops at the watcher's GAME:END.
# Optional and never part of tools/verify.sh: it needs checkouts this repo does not ship.
# Package downloads stay off: the game processes get a dead HTTP(S) proxy, so any request for sbox.game
# fails instead of fetching. Only the join client may reach 127.0.0.1, where the manifest server is.
# Logs: $OUT/server.log, $OUT/client.log, $OUT/client.out (OUT defaults to target/game/<checkout name>).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
CHECKOUT=$(cd -- "${1:?usage: tools/game.sh <checkout> [scene]}" && pwd)
MODE=${MODE:-server}
case $MODE in
    server) IDLE=15 ;;
    join) IDLE=90 ;;
    grubs) IDLE=40 START_CMD=${START_CMD-gr_ffa_next_state} ;;
    render) IDLE=40 ;;
    *) echo "game.sh: unknown MODE '$MODE'" >&2; exit 2 ;;
esac
[ "$MODE" = grubs ] && SCENE=${2:-scenes/beach.scene} || SCENE=${2:-scenes/zoo.scene}
SECS=${SECS:-300}
NAME=$(basename -- "$CHECKOUT")
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${SBOX:-}")
OUT=${OUT:-$ROOT/target/game/$NAME}
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" $$)}
COPIES=/srv/agents/cache/underbox-real-games
COPY=$COPIES/$NAME-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
mkdir -p "$OUT" "$COPIES"
. "$ROOT/tools/game/report.sh"

# Build first, with the network reachable for cargo/NuGet; the game itself then runs with it blocked.
sh "$ROOT/tools/submodules.sh"
case $MODE in server|join) UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" > /dev/null ;; esac
[ "$MODE" = server ] || UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" client > /dev/null
env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4

[ -d "$COPY" ] || cp -a --reflink=auto "$CHECKOUT" "$COPY"
rsync -a --delete --exclude=.git "$CHECKOUT/" "$COPY/"
# Assets/ and assets/ are one folder where a game was authored (Windows) and two here.
if [ -d "$COPY/assets" ] && [ -d "$COPY/Assets" ]; then
    rsync -a --ignore-existing "$COPY/assets/" "$COPY/Assets/"
    rm -rf "$COPY/assets"
fi
# COMPILED_ONLY=1: drop project-JSON sources that ship their _c, so the game reads the shipped
# compiled files rather than ours (engine2 recompiles project JSON resources at mount).
if [ "${COMPILED_ONLY:-0}" = 1 ]; then
    find "$COPY/Assets" -type f \( -name '*.geq' -o -name '*.sdflayer' -o -name '*.surface' -o -name '*.vsnap' \) \
        -exec sh -c 'for f; do [ -f "${f}_c" ] && rm -f "$f"; done' sh {} +
fi
PROJECT=$(python3 -I "$ROOT/tools/game/prepare.py" "$COPY" "$SCENE" "$ROOT/tools/game/RealGameWatch.cs" \
    --idle "$IDLE" --command "${START_CMD:-}")

export HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9
export http_proxy=$HTTP_PROXY https_proxy=$HTTPS_PROXY all_proxy=$ALL_PROXY NO_PROXY= no_proxy=

host_client() {
    OUT=$OUT FRAMES=$1 UNDERBOX_FRAMES_LAST=1 UNTIL='GAME:END' GRACE=3 SHADOW_QUALITY=${SHADOW_QUALITY:-2} \
        sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$PROJECT" ${CLIENT_ARGS:-} > "$OUT/client-boot.txt" 2>&1 || true
}

# The join client: offline package seed, the manifest server on a borrowed port, then +connect.
join_client() {
    hport=$(sh "$ROOT/tools/worktree-port.sh" $$)
    rm -rf "$OUT/seed" "$OUT/serve"
    python3 -I "$ROOT/tools/game/offline_package.py" "$COPY" "$OUT/seed" \
        --manifest-url "http://127.0.0.1:$hport/manifest.json" --manifest-out "$OUT/serve/manifest.json" > /dev/null
    python3 -I -m http.server --bind 127.0.0.1 --directory "$OUT/serve" "$hport" > "$OUT/http.log" 2>&1 &
    http=$!
    NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 SEED=$OUT/seed OUT=$OUT GRACE=3 \
        UNTIL='GAME:PLAYER_STANDING .*local=True|wasn.t found|Unable to download|Kicked|Disconnected' \
        sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- +connect "127.0.0.1:$PORT" ${CLIENT_ARGS:-} > "$OUT/client-boot.txt" 2>&1 || true
    kill "$http" 2>/dev/null || true
}

t0=$(date +%s)
case $MODE in
    render|grubs)
        rm -rf "$OUT/frames"; mkdir -p "$OUT/frames"
        host_client "$OUT/frames"
        LOG=$OUT/client.log
        echo "== $NAME ($MODE) $SCENE: $(( $(date +%s) - t0 )) s"
        report_game "$LOG" client
        grep -hE 'render device' "$OUT/client.out" | head -n 1 || true
        ;;
    server|join)
        # Truncated here, not only by boot-server.sh: the join client must not read a previous run's marker.
        : > "$OUT/server.log"
        # join: the server stops once the joined player stands, with time for the client to see it too.
        [ "$MODE" = join ] && until='GAME:END|GAME:PLAYER_STANDING' grace=20 || until='GAME:END' grace=3
        OUT=$OUT UNTIL=$until GRACE=$grace \
            sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- +game "$PROJECT" +port "$PORT" > "$OUT/boot.txt" 2>&1 &
        SERVER=$!
        if [ "$MODE" = join ]; then
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
        if [ "$MODE" = join ]; then
            report_game "$OUT/client.log" client
            echo "== manifest server requests"; grep -oE '"GET [^"]*" [0-9]+' "$OUT/http.log" || true
        fi
        ;;
esac

echo "== summary"
case $MODE in
    server)
        compile_clean "$LOG" server
        check "player stands" 'GAME:STANDING .*on=[^ ]+' "$LOG"
        check "end reached" 'GAME:END' "$LOG"
        ;;
    join)
        compile_clean "$LOG" server
        check "client found the package offline" 'using what we knew about [^ ]+' "$OUT/client.log"
        refute "no package lookup failure" "wasn.t found|Unable to download package" "$OUT/client.log"
        check "server accepted the client" '[^]]+\] is connected' "$LOG"
        check "server sees the client's player standing" 'GAME:PLAYER_STANDING .*local=False.*' "$LOG"
        check "client's own player stands" 'GAME:PLAYER_STANDING .*local=True.*' "$OUT/client.log"
        ;;
    grubs)
        compile_clean "$LOG" client
        check "round started" 'GAME:COMMAND .*' "$LOG"
        check "sdflayer resources loaded" 'GAME:RESOURCES .*sdflayer=[1-9][0-9]*' "$LOG"
        check "geq resources loaded" 'GAME:RESOURCES .*geq=[1-9][0-9]*' "$LOG"
        check "terrain built" 'GAME:COMPONENTS .*Sdf2DChunk=[1-9][0-9]*' "$LOG"
        check "terrain has collision" 'GAME:PHYSICS bodies=[0-9]+ shapes=[1-9][0-9]*' "$LOG"
        check "grubs spawned" 'GAME:COMPONENTS .* Grub=[1-9][0-9]*' "$LOG"
        check "equipment spawned" 'GAME:COMPONENTS .* Equipment=[1-9][0-9]*' "$LOG"
        check "end reached" 'GAME:END' "$LOG"
        ;;
    render)
        check "end reached" 'GAME:END' "$LOG"
        ;;
esac
[ -f "$OUT/frames/GameWindow.ppm" ] && python3 -I "$ROOT/tools/ppm2png.py" "$OUT/frames/GameWindow.ppm" "$OUT/frame.png" > /dev/null &&
    echo "== frame: $OUT/frame.png"
if [ "$FAILED" = 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL (logs: $OUT)"; exit 1; fi
