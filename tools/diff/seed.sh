#!/bin/sh
# Print a dir of real's compiled core textures, materials and models laid out like a game dir, for
# SEED= on our boots, so both engines see the same content (docs/differential-render.md).
# usage: SEED=$(sh tools/diff/seed.sh <artifact-root>)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
ART=${1:?usage: tools/diff/seed.sh <artifact-root>}
s=$REAL_ENGINE2_RUNS/content3-$(basename -- "$ART")
if [ ! -f "$s/.done" ]; then
    rm -rf "$s"; mkdir -p "$s"
    (cd "$ART/game" && find core -type f \( -name '*.vtex_c' -o -name '*.vmat_c' -o -name '*.vmdl_c' -o -name '*.vmesh_c' \) |
        cpio -pdl --quiet "$s")
    touch "$s/.done"
fi
echo "$s"
