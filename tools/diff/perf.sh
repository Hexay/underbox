#!/bin/sh
# Performance differential: the same scenario on our engine2 and on Facepunch's, CPU time per server
# tick or client frame, boot time and memory (docs/differential-perf.md).
# usage: tools/diff/perf.sh <pile|probe|corpus|game-checkout> [both|ours|real] [artifact-root]   (or tools/diff/run.sh perf ...)
# Env: REPEATS=5, SIDE=server|client (default: client for showroom and skinned, else server), CPUS=0-7
# (taskset), LP_NUM_THREADS=4, WARMUP_TICKS=250 TICKS=500, WARMUP_FRAMES=60 FRAMES=200, SECS=300,
# SCENE=scenes/x.scene, CONTENT=source (no real core content seeded into ours), GPU=1 (no lavapipe ICD),
# UNDERBOX_ENGINE2=<libengine2.so> (else a release build).
# Out: target/diff/perf/<name>/<engine>-<i>/{sample.json,...}, report.txt.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
. "$ROOT/tools/cache-dir.sh"
SRC=${1:?usage: tools/diff/perf.sh <probe|corpus|game-checkout> [both|ours|real] [artifact-root]}
WHICH=${2:-both}
ART=${3:-$(ls -td "$REAL_ENGINE2_CACHE"/*/game/bin/linuxsteamrt64 2>/dev/null | head -n 1 | sed 's#/game/bin/linuxsteamrt64$##')}
case $SRC in probe|pile) SRC=$ROOT/tools/diff/$SRC ;; esac
[ -d "$SRC" ] || SRC=$ROOT/corpus/$SRC
[ -d "$SRC" ] || { echo "perf: no corpus or checkout '$1'" >&2; exit 2; }
SRC=$(cd -- "$SRC" && pwd)
NAME=$(basename -- "$SRC")
case $NAME in showroom|skinned) SIDE=${SIDE:-client} ;; *) SIDE=${SIDE:-server} ;; esac
REPEATS=${REPEATS:-5}
CPUS=${CPUS:-0-7}
SECS=${SECS:-300}
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
OUT=$ROOT/target/diff/perf/$NAME
[ "$SIDE" = server ] || OUT=$OUT-$SIDE
rm -rf "$OUT"; mkdir -p "$OUT" "$UNDERBOX_GAME_COPIES"
export LP_NUM_THREADS=${LP_NUM_THREADS:-4}
# Real validates no indirect args; ours does by default (docs/perf-render.md). =1 measures that cost.
export WGPU_VALIDATION_INDIRECT_CALL=${WGPU_VALIDATION_INDIRECT_CALL:-0}
# A release engine2 against real's release build; UNDERBOX_ENGINE2=<so> uses that one instead.
if [ -z "${UNDERBOX_ENGINE2:-}" ]; then
    echo "== building release engine2"
    env -u CARGO_INCREMENTAL cargo build -q --release --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4
    export UNDERBOX_ENGINE2="${CARGO_TARGET_DIR:-$ROOT/target}/release/libengine2.so"
fi
if [ "${GPU:-}" != 1 ] && [ -z "${VK_ICD_FILENAMES:-}" ]; then
    # Both engines on the same lavapipe: real-env.sh's choice, made here so ours gets it too.
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
    for f in "$UNDERBOX_CACHE"/mesa-lvp/install/share/vulkan/icd.d/lvp_icd.*.json; do [ -f "$f" ] && VK_ICD_FILENAMES=$f; done
    export VK_ICD_FILENAMES
fi

prepare() {
    copy=$UNDERBOX_GAME_COPIES/$NAME-perf-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
    sh "$ROOT/tools/game/copy.sh" "$SRC" "$copy"
    mode=tick; [ "$SIDE" = server ] || mode=frame
    python3 -I "$ROOT/tools/game/prepare.py" "$copy" "${SCENE:-}" "$ROOT/tools/diff/PerfMark.cs" --prop "Mode=$mode" \
        --prop "WarmupTicks=${WARMUP_TICKS:-250}" --prop "Ticks=${TICKS:-500}" \
        --prop "WarmupFrames=${WARMUP_FRAMES:-60}" --prop "Frames=${FRAMES:-200}"
}

# boot <engine> <i>: one sampled run; the sampler stays off the engine's cores where it can.
boot() {
    eng=$1 dir=$OUT/$1-$2 real= seed=
    [ "$eng" = ours ] || real=$ART
    echo "== $NAME $SIDE on $eng, run $2 (cpus $CPUS, LP_NUM_THREADS=$LP_NUM_THREADS, load $(cut -d' ' -f1-3 /proc/loadavg))"
    PROJECT=$(prepare)
    [ -n "$real" ] || [ "${CONTENT:-real}" = source ] || [ -z "$ART" ] || seed=$(sh "$ROOT/tools/diff/seed.sh" "$ART")
    export UNDERBOX_LOCAL_PROJECT="${PROJECT%/*}"
    mkdir -p "$dir"
    cut -d' ' -f1-3 /proc/loadavg > "$dir/load.txt"
    until='PERF:DONE|Error \| .*PerfMark'
    if [ "$SIDE" = server ]; then
        python3 -I "$ROOT/tools/diff/perf_sample.py" "$dir/server.log" "$dir/server.log" "$dir/sample.json" &
        SAMPLER=$!
        UNDERBOX_REAL_ENGINE2=$real SEED=$seed OUT=$dir UNTIL="$until" GRACE=1 \
            taskset -c "$CPUS" sh "$ROOT/tools/boot-server.sh" "$SECS" "$SBOX" -- +game "$PROJECT" > "$dir/boot.txt" 2>&1 || true
    else
        python3 -I "$ROOT/tools/diff/perf_sample.py" "$dir/client.out" "@$dir/client.logpath" "$dir/sample.json" &
        SAMPLER=$!
        # Real's window is the screen's size with no WM; ours renders 1024x768 offscreen (frame.sh).
        XVFB=
        if [ -n "$real" ]; then
            d=:$(( 200 + $$ % 700 ))
            Xvfb "$d" -screen 0 1024x768x24 -nocursor -nolisten tcp > "$dir/xvfb.log" 2>&1 &
            XVFB=$!
            export DISPLAY=$d
            # SDL's ALSA backend spins on boot-client's null PCM (one core); dummy drains in real time,
            # like ours' null device.
            export SDL_AUDIO_DRIVER=${PERF_SDL_AUDIO:-dummy}
        fi
        # FRAMES= turns off ours' per-frame readback and hash; real never does it.
        FRAMES= UNDERBOX_STANDIN_FRIENDS=1 UNDERBOX_REAL_ENGINE2=$real SEED=$seed OUT=$dir UNTIL="$until" GRACE=1 \
            SHADOW_QUALITY=${SHADOW_QUALITY:-2} FOG_QUALITY=${FOG_QUALITY:--1} \
            taskset -c "$CPUS" sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$PROJECT" > "$dir/boot.txt" 2>&1 || true
        [ -z "$XVFB" ] || kill "$XVFB" 2>/dev/null || true
        unset DISPLAY SDL_AUDIO_DRIVER
    fi
    kill "$SAMPLER" 2>/dev/null || true
    wait "$SAMPLER" 2>/dev/null || true
    cut -d' ' -f1-3 /proc/loadavg >> "$dir/load.txt"
    if grep '^FAIL: the game downloaded' "$dir/boot.txt"; then exit 4; fi
    [ -s "$dir/sample.json" ] || echo "perf: $eng run $2 left no sample (see $dir/boot.txt)"
}

need_real() { [ -n "$ART" ] || { echo "perf: no real engine2; run tools/diff/fetch.sh" >&2; exit 3; }; }
cut -d' ' -f1-3 /proc/loadavg > "$OUT/load.txt"
i=1
while [ "$i" -le "$REPEATS" ]; do
    case $WHICH in
        both) need_real; boot ours "$i"; boot real "$i" ;;
        ours) boot ours "$i" ;;
        real) need_real; boot real "$i" ;;
        *) echo "perf: unknown engine '$WHICH'" >&2; exit 2 ;;
    esac
    i=$((i + 1))
done
cut -d' ' -f1-3 /proc/loadavg >> "$OUT/load.txt"
python3 -I "$ROOT/tools/diff/perf_report.py" "$OUT" "$SIDE" "cpus=$CPUS LP_NUM_THREADS=$LP_NUM_THREADS icd=${VK_ICD_FILENAMES:-none}" | tee "$OUT/report.txt"
