#!/bin/sh
# Print a game dir that runs Facepunch's real engine2: a reflinked copy of <game-dir> (from
# tools/game-dir.sh) whose bin/linuxsteamrt64 is the downloaded one. Separate from our copies:
# install-engine2.sh keeps a real libsteam_api.so, so mixing would leak it into our boots.
# usage: GAME=$(sh tools/diff/real-game.sh <game-dir> <artifact-root from tools/diff/fetch.sh> <owner-pid> [server args])
# The copy lives while <owner-pid> does. It gets the artifact's compiled game/core too, as a real
# install has, and any fetched compiled addons (REAL_CORE=0 keeps the source-only core and addons both
# engines otherwise see).
# A `+game <x>.sbproj` project gets its GameResources and `.vmat` sources compiled first: real engine2
# doesn't compile at mount (the editor does), see docs/differential-render.md.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
SRC=$1
ART=$2
OWNER=$3
shift 3
. "$ROOT/tools/diff/env.sh"
RUNS=$REAL_ENGINE2_RUNS
while [ $# -gt 1 ]; do
    if [ "$1" = +game ] && [ -d "$(dirname -- "$2")/Assets" ]; then
        env -u CARGO_INCREMENTAL cargo run -q --manifest-path "$ROOT/Cargo.toml" -p assets --bin assetc -j4 -- \
            --materials "$(dirname -- "$2")/Assets" >&2
    fi
    shift
done
[ -f "$ART/game/bin/linuxsteamrt64/libengine2.so" ] || { echo "real-game: no libengine2.so under $ART" >&2; exit 3; }
mkdir -p "$RUNS"
for d in "$RUNS"/run-*; do
    [ -d "$d" ] && ! kill -0 "${d##*-}" 2>/dev/null && rm -rf "$d"
done
G=$RUNS/run-$OWNER
rm -rf "$G"
mkdir -p "$G"
find "$SRC" -mindepth 1 -maxdepth 1 ! -name logs ! -name '.underbox-lease.*' -exec cp -a --reflink=auto -t "$G" {} +
mkdir -p "$G/logs"
rm -rf "$G/bin/linuxsteamrt64"
cp -a --reflink=auto "$ART/game/bin/linuxsteamrt64" "$G/bin/"
if [ "${REAL_CORE:-1}" = 1 ]; then
    cp -a --reflink=auto "$ART/game/core/." "$G/core/"
    [ ! -d "$ART/game/addons" ] || cp -a --reflink=auto "$ART/game/addons/." "$G/addons/"
fi
# bin/managed links point at ../linuxsteamrt64 (install-engine2.sh); steam_api64 needs one too.
[ -d "$G/bin/managed" ] && ln -sfn ../linuxsteamrt64/libsteam_api.so "$G/bin/managed/libsteam_api64.so"
echo "$G"
