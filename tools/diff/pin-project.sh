#!/bin/sh
# Sync a game checkout into a scratch copy that starts in SCENE with tools/diff/FramePin.cs pinning camera and
# time, and print the copy's .sbproj (docs/differential-render.md "Real games"). The checkout stays read-only.
# usage: tools/diff/pin-project.sh <checkout> <tag>   (tag keeps frame and gpu copies apart)
# Env: SCENE, CAMERA_POS, CAMERA_ANGLES, CAMERA_FOV, FREEZE_AT, KEEP_PARTICLES, KEEP_HUD (FramePin's props).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/cache-dir.sh"
CHECKOUT=$(cd -- "${1:?usage: tools/diff/pin-project.sh <checkout> <tag>}" && pwd)
COPY=$UNDERBOX_GAME_COPIES/$(basename -- "$CHECKOUT")-${2:?tag}-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
mkdir -p "$UNDERBOX_GAME_COPIES"
sh "$ROOT/tools/game/copy.sh" "$CHECKOUT" "$COPY" >&2
python3 -I "$ROOT/tools/game/prepare.py" "$COPY" "${SCENE:-}" "$ROOT/tools/diff/FramePin.cs" --stub-lobby \
    --prop "Position=${CAMERA_POS:-}" --prop "Angles=${CAMERA_ANGLES:-0,0,0}" --prop "FieldOfView=${CAMERA_FOV:-0}" \
    --prop "FreezeAt=${FREEZE_AT:-0}" --prop "KeepParticles=${KEEP_PARTICLES:-false}" --prop "KeepHud=${KEEP_HUD:-false}"
