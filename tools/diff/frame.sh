#!/bin/sh
# Render one corpus scene on our engine2 and on Facepunch's, then compare the frames.
# usage: [MANAGED=1] tools/diff/frame.sh <corpus> [both|ours|real] [artifact-root]   (or tools/diff/run.sh frame ...)
# Out: target/diff/frame/<corpus>/{ours,real}.png, diff.png, metrics.txt (tools/diff/frame.py).
# MANAGED=1 renders both through managed's scene renderer (+r_managed_scene 1), into <corpus>-managed.
# A game checkout in place of <corpus> (docs/differential-render.md "Real games"): [SCENE=scenes/x.scene]
# [CAMERA_POS=x,y,z CAMERA_ANGLES=p,y,r CAMERA_FOV=deg] [FREEZE_AT=scene-s] [KEEP_PARTICLES=true] [KEEP_HUD=true].
# Both sides see the same content: real's compiled core textures, materials and models are seeded
# into our game dir for the boot (CONTENT=source skips that). Shaders stay each engine's own.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
CORPUS=${1:?usage: tools/diff/frame.sh <corpus|game-checkout> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-}
PROJECT=
if [ -d "$CORPUS" ]; then
    # A game checkout: a scratch copy starting in SCENE with tools/diff/FramePin.cs pinning camera and time.
    . "$ROOT/tools/cache-dir.sh"
    CHECKOUT=$(cd -- "$CORPUS" && pwd)
    CORPUS=$(basename -- "$CHECKOUT")
    COPY=$UNDERBOX_GAME_COPIES/$CORPUS-frame-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
    mkdir -p "$UNDERBOX_GAME_COPIES"
    sh "$ROOT/tools/game/copy.sh" "$CHECKOUT" "$COPY"
    PROJECT=$(python3 -I "$ROOT/tools/game/prepare.py" "$COPY" "${SCENE:-}" "$ROOT/tools/diff/FramePin.cs" --stub-lobby \
        --prop "Position=${CAMERA_POS:-}" --prop "Angles=${CAMERA_ANGLES:-0,0,0}" --prop "FieldOfView=${CAMERA_FOV:-0}" \
        --prop "FreezeAt=${FREEZE_AT:-0}" --prop "KeepParticles=${KEEP_PARTICLES:-false}" --prop "KeepHud=${KEEP_HUD:-false}")
    export PROJECT
fi
OUT=$ROOT/target/diff/frame/$CORPUS
[ "${MANAGED:-}" = 1 ] && OUT=$OUT-managed
# Both engines get the auto-detected Low preset's fog quality (Off); shadows stay pinned High.
export FOG_QUALITY=${FOG_QUALITY:--1}
mkdir -p "$OUT"
[ -n "$ART" ] || ART=$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')
[ -n "$ART" ] || { echo "frame: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }

ours() {
    echo "== $CORPUS on ours"
    s=
    [ "${CONTENT:-real}" = source ] || s=$(sh "$ROOT/tools/diff/seed.sh" "$ART")
    SEED=$s CORPUS=$CORPUS OUT="$OUT/ours" sh "$ROOT/tools/client-frame.sh" > "$OUT/ours.txt" 2>&1 || true
    tail -n 3 "$OUT/ours.txt"
}

real() {
    echo "== $CORPUS on real"
    sh "$ROOT/tools/diff/real-client.sh" "$CORPUS" "$OUT/real" "${SECS:-300}" "$ART" > "$OUT/real.txt" 2>&1 || true
    tail -n 3 "$OUT/real.txt"
}

case $WHICH in
    both) ours; real ;;
    ours) ours ;;
    real) real ;;
    *) echo "frame: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
A=$OUT/ours/frames/GameWindow.ppm
B=$OUT/real/frame.xwd
[ -s "$A" ] && [ -s "$B" ] || { echo "RESULT: FAIL (missing frame: $A or $B)"; exit 1; }
python3 -I "$ROOT/tools/diff/frame.py" "$A" "$B" "$OUT" | tee "$OUT/report.txt"
