#!/bin/sh
# Host corpus/nethost on our engine, connect the gns probe client once it's listening, summarise both.
# usage: tools/net-probe.sh [hold-seconds=5] [sbox-public]
# Logs: target/net-probe/{server.log,probe.log}
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
HOLD=${1:-5}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
OUT=$ROOT/target/net-probe
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" $$)}
mkdir -p "$OUT"
# see docs/dev-loop.md (incremental)
env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p gns --example probe -j4

: > "$OUT/server.log"
OUT=$OUT sh "$ROOT/tools/boot-server.sh" $((HOLD + 60)) "$SBOX" -- \
    +game "$ROOT/corpus/nethost/.sbproj" +port $PORT > /dev/null 2>&1 &
BOOT=$!
# Generous: covers a wait for the shared-build lock (tools/boot-lock.sh) and a dotnet rebuild.
deadline=$(( $(date +%s) + 720 ))
until grep -q 'Server started' "$OUT/server.log" 2>/dev/null; do
    [ "$(date +%s)" -lt "$deadline" ] || { echo "server never started"; break; }
    sleep 1
done
env -u CARGO_INCREMENTAL cargo run -q --manifest-path "$ROOT/Cargo.toml" -p gns --example probe -- "127.0.0.1:$PORT" "$HOLD" > "$OUT/probe.log" 2>&1 || true
pkill -TERM -f "sbox-server.dll .*[+]port $PORT" 2>/dev/null || true
wait $BOOT || true

echo "== probe"; tail -n 12 "$OUT/probe.log"
echo "== server"; grep -E '^[0-9]{2}:' "$OUT/server.log" | grep -vE 'RenderQu|mount/|Welcome|Submit any' | sed -E 's/^[0-9:]+ //' | cut -c1-200 | sort | uniq -c | sort -rn | head -n 20
echo "== stubs: $(grep -oE 'stub: [A-Za-z0-9_]+' "$OUT/server.log" | sort -u | sed 's/stub: //' | tr '\n' ' ')"
