#!/bin/sh
# Boot a real game from a checkout (Facepunch/sandbox, apetavern/grubs) on our libengine2 and summarise.
# usage: [MODE=server|join|render] [SECS=300] [SBOX=<sbox-public>] [COMPILED_ONLY=1] tools/game.sh <checkout> [scene=scenes/zoo.scene]
#   server  dedicated server on the scene until its bodies settle and it has idled (default)
#   join    the server plus a headless client connecting to it (see docs/real-games.md: stops at the
#           client's package lookup, which needs a patch or the published package)
#   render  a headless client hosting the scene itself; its last frame goes to $OUT/frame.png
# The checkout stays read-only: runs happen in a scratch copy under /srv/agents/cache, which gets the
# watcher tools/game/RealGameWatch.cs and the chosen startup scene. Stops at the watcher's GAME:END.
# Optional and never part of tools/verify.sh: it needs checkouts this repo does not ship.
# Package downloads stay off: the game process gets a dead HTTP(S) proxy, so a game that asks for a
# package from sbox.game fails that request instead of fetching it.
# Logs: $OUT/server.log, $OUT/client.log (OUT defaults to target/game/<checkout name>).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
CHECKOUT=$(cd -- "${1:?usage: tools/game.sh <checkout> [scene]}" && pwd)
SCENE=${2:-scenes/zoo.scene}
MODE=${MODE:-server}
SECS=${SECS:-300}
NAME=$(basename -- "$CHECKOUT")
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${SBOX:-}")
OUT=${OUT:-$ROOT/target/game/$NAME}
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" 2)}
COPIES=/srv/agents/cache/underbox-real-games
COPY=$COPIES/$NAME-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
mkdir -p "$OUT" "$COPIES"
case $MODE in server) IDLE=15 ;; join) IDLE=150 ;; render) IDLE=40 ;; *) echo "game.sh: unknown MODE '$MODE'" >&2; exit 2 ;; esac

# Build first, with the network reachable for cargo/NuGet; the game itself then runs with it blocked.
sh "$ROOT/tools/submodules.sh"
[ "$MODE" = render ] || UNDERBOX_LEASE=$$ sh "$ROOT/tools/game-dir.sh" "$SBOX" > /dev/null
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
PROJECT=$(python3 -I "$ROOT/tools/game/prepare.py" "$COPY" "$SCENE" "$ROOT/tools/game/RealGameWatch.cs" --idle "$IDLE")

export HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9
export http_proxy=$HTTP_PROXY https_proxy=$HTTPS_PROXY all_proxy=$ALL_PROXY NO_PROXY= no_proxy=

summary() { grep -iE 'exception|error|panick' "$1" | grep -vE 'stub:|GAME:|Error \||achievements' | sed -E 's/^[0-9:]+ //' | cut -c1-200 | sort | uniq -c | sort -rn | head -n "$2" || true; }
report_game() {
    echo "== compile: $(grep -c "Compile of .* Failed" "$1" || true) failed, $(grep -cE ' Error \| ' "$1" || true) errors"
    grep -E ' Error \| ' "$1" | sed -E 's/^.* Error \| //; s/ - \/.*//' | cut -c1-200 | sort -u | head -n 10 || true
    echo "== missing components:"; grep -oE 'Missing Component: .*' "$1" | cut -c1-140 | sort | uniq -c | sort -rn | head -n 10 || true
    echo "== packages refused: $(grep -c 'Package not found:' "$1" || true), HTTP stopped at the dead proxy: $(grep -c '127\.0\.0\.1:9' "$1" || true)"
    echo "== markers"; grep -oE 'GAME:(START|AWAKE|TRACE|SETTLED|DROP|STANDING|END).*' "$1" | cut -c1-200
    echo "== last tick"; grep -oE 'GAME:TICK.*' "$1" | tail -n 1 | cut -c1-300 || true
    echo "== errors"; summary "$1" 15
    echo "== stubs hit: $(grep -oE 'stub: [A-Za-z0-9_]+' "$1" | sort -u | wc -l)"
}

t0=$(date +%s)
if [ "$MODE" = render ]; then
    mkdir -p "$OUT/frames"
    OUT=$OUT FRAMES=$OUT/frames UNDERBOX_FRAMES_LAST=1 UNTIL='GAME:END' GRACE=3 SHADOW_QUALITY=${SHADOW_QUALITY:-2} \
        sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$PROJECT" > "$OUT/client-boot.txt" 2>&1 || true
    LOG=$OUT/client.log
    echo "== $NAME (render) $SCENE: $(( $(date +%s) - t0 )) s"
    report_game "$LOG"
    grep -hE 'render device' "$OUT/client.out" | head -n 1 || true
    tail -n 8 "$OUT/client-boot.txt" | cut -c1-200
    [ -f "$OUT/frames/GameWindow.ppm" ] && python3 -I "$ROOT/tools/ppm2png.py" "$OUT/frames/GameWindow.ppm" "$OUT/frame.png" > /dev/null && echo "== frame: $OUT/frame.png"
else
    # Truncated here, not only by boot-server.sh: run_client must not read a previous run's marker.
    : > "$OUT/server.log"
    OUT=$OUT UNTIL='GAME:END' GRACE=3 sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- \
        +game "$PROJECT" +port "$PORT" > "$OUT/boot.txt" 2>&1 &
    SERVER=$!
    if [ "$MODE" = join ]; then
        (
            until grep -q 'GAME:SETTLED' "$OUT/server.log" 2>/dev/null; do
                kill -0 "$SERVER" 2>/dev/null || exit 0
                sleep 1
            done
            OUT=$OUT UNTIL='wasn.t found|GAME:STANDING|Disconnect' GRACE=5 \
                sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- +connect "127.0.0.1:$PORT" > "$OUT/client-boot.txt" 2>&1 || true
        ) &
        CLIENT=$!
    fi
    wait "$SERVER" || true
    if [ "$MODE" = join ]; then wait "$CLIENT" || true; fi
    LOG=$OUT/server.log
    echo "== $NAME ($MODE) $SCENE: $(( $(date +%s) - t0 )) s, port $PORT"
    report_game "$LOG"
    if [ "$MODE" = join ]; then
        echo "== client connect"; grep -hE 'wasn.t found|Connected|Disconnect|Connecting|Handshake|Game Package|connect' "$OUT/client.log" "$OUT/client.out" 2>/dev/null | cut -c1-180 | awk '!s[$0]++' | head -n 12 || true
        echo "== server connections"; grep -E 'Connect|Handshake|client' "$LOG" | grep -v 'GAME:' | cut -c1-180 | awk '!s[$0]++' | head -n 8 || true
    fi
fi
if grep -q 'GAME:END' "$LOG"; then echo "RESULT: reached GAME:END"; else echo "RESULT: no GAME:END (log: $LOG)"; exit 1; fi
