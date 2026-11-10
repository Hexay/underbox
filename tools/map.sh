#!/bin/sh
# Map loading: host corpus/mapworld headless (a MapInstance on maps/tiny.vpk, the world crate's
# fixture map) and assert MapProbe's MAP: markers: entities spawned from the KV3 lump and the
# legacy-blob child lump, and the world nodes' scene objects.
# usage: tools/map.sh [seconds=90] [sbox-public]
# Log: target/map/server.log. Exit 0 only when every marker is present.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-90}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "${2:-}")
OUT=$ROOT/target/map
LOG=$OUT/server.log
PORT=${PORT:-$(sh "$ROOT/tools/worktree-port.sh" 0)}
mkdir -p "$OUT"

OUT=$OUT UNTIL='MAP:END' GRACE=1 sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- \
    +game "$ROOT/corpus/mapworld/.sbproj" +port "$PORT" > "$OUT/boot.txt" 2>&1 || true

fail=0
check() {
    if grep -qE "MAP:$1" "$LOG"; then echo "PASS $2"; else echo "FAIL $2"; fail=1; fi
}
check 'LOADED True' 'map loaded'
check 'ENTITY info_player_start <spawn_a> @ 0(\.0+)?,-100' 'spawn point placed from the KV3 lump'
check 'ENTITY light_omni <lamp>' 'light entity'
check 'ENTITY prop_dynamic <crate>' 'prop entity'
check 'ENTITY info_target <child_target> @ 10(\.0+)?,20(\.0+)?,30' 'child lump entity (hashed keys)'
check 'SPAWNPOINTS 1' 'SpawnPoint component made'
check 'LIGHTS 1' 'PointLight component made'
check 'SCENEOBJECT models/dev/plane.vmdl' 'world node plane'
check 'SCENEOBJECT models/dev/box.vmdl @ 60(\.0+)?,40' 'second world node box'
check 'SCENEOBJECTS [4-9]' 'world objects plus the prop'
check 'END' 'probe finished'

echo "== markers"; grep -oE 'MAP:.*' "$LOG" | cut -c1-120 | head -n 20 || true
echo "== errors"; grep -iE 'exception|error|world ' "$LOG" | grep -v 'stub:' | cut -c1-200 | sort -u | head -n 8 || true
if [ $fail -eq 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL (log: $LOG)"; fi
exit $fail
