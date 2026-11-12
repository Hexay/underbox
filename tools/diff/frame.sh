#!/bin/sh
# Render one corpus scene on our engine2 and on Facepunch's, then compare the frames.
# usage: [MANAGED=1] tools/diff/frame.sh <corpus> [both|ours|real] [artifact-root]   (or tools/diff/run.sh frame ...)
# Out: target/diff/frame/<corpus>/{ours,real}.png, diff.png, metrics.txt (tools/diff/frame.py).
# MANAGED=1 renders both through managed's scene renderer (+r_managed_scene 1), into <corpus>-managed.
# Both sides see the same content: real's compiled core textures, materials and models are seeded
# into our game dir for the boot (CONTENT=source skips that). Shaders stay each engine's own.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
CORPUS=${1:?usage: tools/diff/frame.sh <corpus> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-}
OUT=$ROOT/target/diff/frame/$CORPUS
[ "${MANAGED:-}" = 1 ] && OUT=$OUT-managed
# Both engines get the auto-detected Low preset's fog quality (Off); shadows stay pinned High.
export FOG_QUALITY=${FOG_QUALITY:--1}
mkdir -p "$OUT"
[ -n "$ART" ] || ART=$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')
[ -n "$ART" ] || { echo "frame: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }

seed() {
    s=$REAL_ENGINE2_RUNS/content3-$(basename -- "$ART")
    if [ ! -f "$s/.done" ]; then
        rm -rf "$s"; mkdir -p "$s"
        (cd "$ART/game" && find core -type f \( -name '*.vtex_c' -o -name '*.vmat_c' -o -name '*.vmdl_c' -o -name '*.vmesh_c' \) |
            cpio -pdl --quiet "$s")
        touch "$s/.done"
    fi
    echo "$s"
}

ours() {
    echo "== $CORPUS on ours"
    s=
    [ "${CONTENT:-real}" = source ] || s=$(seed)
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
