#!/bin/sh
# Capture one pinned frame's Vulkan work on our engine2 and on Facepunch's through the capture layer
# (tools/diff/gpu/), list it per pass and draw, and report where the two first diverge. See docs/differential-gpu.md.
# usage: tools/diff/gpu.sh <corpus|game-checkout> [both|ours|real] [artifact-root]   (or tools/diff/run.sh gpu ...)
# Env: GRACE (s after game.load before the trigger, default 20), SECS (per boot, 300), DUMP=<n>[:<draw>]
# (copy out pass P<n>'s attachments, as of that draw; numbering as in the side's listing; DUMP_OURS and
# DUMP_REAL set one side), MANAGED=1, UNDERBOX_GPU_VERTS=<n> (each draw's first n vertices as "vvb"),
# UNDERBOX_GPU_STORE_MAX=<bytes> (keep storage buffers up to that size, default 512 KiB), plus
# frame.sh's game-checkout pins.
# Out: target/diff/gpu/<name>/{ours,real}.txt (listings), report.txt, dumps/*.png; raw captures in
# $UNDERBOX_CACHE/underbox-gpu/<worktree>/<name>/<side>/.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/cache-dir.sh"
. "$ROOT/tools/diff/env.sh"
SRC=${1:?usage: tools/diff/gpu.sh <corpus|game-checkout> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
if [ -d "$SRC" ]; then
    PROJECT=$(sh "$ROOT/tools/diff/pin-project.sh" "$SRC" gpu)
    NAME=$(basename -- "$(cd -- "$SRC" && pwd)")
else
    PROJECT=$ROOT/corpus/$SRC/.sbproj
    NAME=$SRC
    [ -f "$PROJECT" ] || { echo "gpu: no corpus or checkout '$SRC'" >&2; exit 2; }
fi
[ "${MANAGED:-}" = 1 ] && NAME=$NAME-managed
OUT=$ROOT/target/diff/gpu/$NAME
CAP=$UNDERBOX_CACHE/underbox-gpu/$(basename -- "$ROOT")/$NAME
mkdir -p "$OUT" "$CAP"
# Real validates no indirect args; wgpu's check would add a dispatch and copy per indirect pass.
export WGPU_VALIDATION_INDIRECT_CALL=${WGPU_VALIDATION_INDIRECT_CALL:-0}
LAYER=$(sh "$ROOT/tools/diff/gpu-layer.sh")
# Real's textures are named by matching their content against the seeded core (gpu_vtex.py).
CORE=${ART:+$ART/game/core}
ICD=$(ls "$UNDERBOX_CACHE"/mesa-lvp/install/share/vulkan/icd.d/lvp_icd.*.json 2>/dev/null | head -n 1)
[ -n "$ICD" ] || { echo "gpu: no local lavapipe; run tools/diff/lavapipe.sh" >&2; exit 3; }

# side <ours|real>: one boot on its own Xvfb; a watcher touches the trigger GRACE s after the game loads.
side() {
    s=$1 dir=$CAP/$1 out=$OUT/$1
    echo "== $NAME on $s"
    rm -rf "$dir" "$out"
    mkdir -p "$dir" "$out"
    d=:$(( 200 + ($$ + ${#s}) % 700 ))
    Xvfb "$d" -screen 0 1024x768x24 -nocursor -nolisten tcp > "$out/xvfb.log" 2>&1 &
    xvfb=$!
    (
        t=0
        until [ -s "$out/client.logpath" ] && grep -q 'game.load success' "$(cat "$out/client.logpath")" 2>/dev/null; do
            sleep 1; t=$((t + 1)); [ "$t" -lt "${SECS:-300}" ] || exit 0
        done
        sleep "${GRACE:-20}"
        touch "$dir/trigger"
    ) &
    watcher=$!
    real= seed= args=
    if [ "$s" = real ]; then real=$ART; else seed=$(sh "$ROOT/tools/diff/seed.sh" "$ART"); fi
    [ "${MANAGED:-}" = 1 ] && args="+r_managed_scene 1"
    if [ "$s" = real ]; then dump=${DUMP_REAL:-${DUMP:-}}; else dump=${DUMP_OURS:-${DUMP:-}}; fi
    # shellcheck disable=SC2086
    DISPLAY=$d SDL_VIDEO_DRIVER=x11 VK_ICD_FILENAMES=$ICD WGPU_DEBUG=1 \
        VK_ADD_LAYER_PATH=$LAYER VK_INSTANCE_LAYERS=VK_LAYER_UNDERBOX_gpu_capture UNDERBOX_GPU_DIR=$dir \
        UNDERBOX_GPU_DUMP=$dump UNDERBOX_STANDIN_FRIENDS=1 UNDERBOX_REAL_ENGINE2=$real SEED=$seed \
        OUT=$out UNTIL='gpu-capture: (done|failed)' GRACE=1 SHADOW_QUALITY=2 FOG_QUALITY=-1 \
        sh "$ROOT/tools/boot-client.sh" "${SECS:-300}" "$SBOX" -- -mcp +game "$PROJECT" $args > "$out/boot.txt" 2>&1 || true
    kill "$watcher" "$xvfb" 2>/dev/null || true
    grep -h '^FAIL: the game downloaded' "$out/boot.txt" && exit 4
    [ -z "$(find "$UNDERBOX_CACHE" -name '*.webstream' -print -quit)" ] || { echo "gpu: a .webstream appeared under \$UNDERBOX_CACHE" >&2; exit 4; }
    grep -h 'gpu-capture:' "$out/client.out" | tail -n 2 || true
    [ -f "$dir/done" ] || { echo "gpu: $s captured no frame (see $out/boot.txt)"; return 0; }
    python3 -I "$ROOT/tools/diff/gpu.py" list "$dir/frame.jsonl" "$CORE" > "$OUT/$s.txt"
    python3 -I "$ROOT/tools/diff/gpu.py" png "$dir" "$OUT/dumps/$s"
    echo "$s: $(tail -n 1 "$dir/frame.jsonl" | cut -c1-120); listing $OUT/$s.txt"
}

case $WHICH in
    both) [ -n "$ART" ] || { echo "gpu: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; side ours; side real ;;
    ours) side ours ;;
    real) [ -n "$ART" ] || { echo "gpu: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; side real ;;
    *) echo "gpu: unknown engine '$WHICH'" >&2; exit 2 ;;
esac
[ -f "$CAP/ours/done" ] && [ -f "$CAP/real/done" ] || exit 0
python3 -I "$ROOT/tools/diff/gpu.py" diff "$CAP/ours/frame.jsonl" "$CAP/real/frame.jsonl" "$CORE" > "$OUT/report.txt"
head -n "${N:-40}" "$OUT/report.txt"
echo "gpu: full report in $OUT/report.txt"
