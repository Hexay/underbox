#!/bin/sh
# Golden client frame: boot the client headless on corpus/showroom (camera, light, dev boxes on a
# plane), let it render GRACE s (default 6) past `game.load success`, and check that the last
# presented frames settled on one hash and that it matches corpus/showroom/golden.tsv for this
# adapter. The frame is written as $OUT/frame.png (OUT defaults to target/client-frame).
# usage: [UNDERBOX_BLESS=1] [CORPUS=showroom] [MANAGED=1] tools/client-frame.sh [max-s=240] [sbox-public]
# CORPUS picks another corpus project with its own golden.tsv (mapworld: a loaded map).
# MANAGED=1 draws through managed's scene renderer (+r_managed_scene 1), checked against
# golden-managed.tsv, frame in target/client-frame-managed.
# UNDERBOX_BLESS=1 records the hash; an adapter without a recorded hash is skipped (exit 0).
# Don't wrap this in flock: boot-client.sh runs in this worktree's own game dir (docs/dev-loop.md).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
CORPUS=${CORPUS:-showroom}
if [ "${MANAGED:-}" = 1 ]; then SUFFIX=-managed; set -- "${1:-240}" "${2:-}" +r_managed_scene 1; else SUFFIX=; set -- "${1:-240}" "${2:-}"; fi
OUT=${OUT:-$ROOT/target/client-frame$SUFFIX}
GOLDEN=$ROOT/corpus/$CORPUS/golden$SUFFIX.tsv
SETTLE=5
rm -rf "$OUT"; mkdir -p "$OUT/frames"
SECS=$1 SBOX=$2; shift 2
OUT="$OUT" FRAMES="$OUT/frames" UNDERBOX_FRAMES_LAST=1 UNTIL='game.load success' GRACE=${GRACE:-6} \
    sh "$ROOT/tools/boot-client.sh" "$SECS" "$SBOX" -- -mcp +game "$ROOT/corpus/$CORPUS/.sbproj" "$@" > "$OUT/boot.txt" 2>&1 || true
ADAPTER=$(grep -ohE 'render device: .*' "$OUT/client.out" | head -n 1 | sed 's/^render device: //')
[ -n "$ADAPTER" ] || { tail -n 20 "$OUT/boot.txt"; echo "RESULT: FAIL (no render device)"; exit 1; }
HASH=$(tail -n 1 "$OUT/frames.tsv" | cut -f2)
SETTLED=$(tail -n "$SETTLE" "$OUT/frames.tsv" | cut -f2 | sort -u | wc -l)
python3 -I "$ROOT/tools/ppm2png.py" "$OUT/frames/GameWindow.ppm" "$OUT/frame.png" > /dev/null
echo "== $(wc -l < "$OUT/frames.tsv") frames; last $(tail -n 1 "$OUT/frames.tsv" | cut -f1) $HASH on $ADAPTER"
echo "== frame: $OUT/frame.png"
[ "$SETTLED" -eq 1 ] || { tail -n "$SETTLE" "$OUT/frames.tsv"; echo "RESULT: FAIL (last $SETTLE frames differ)"; exit 1; }
WANT=$(awk -F'\t' -v a="$ADAPTER" '$1 == a { print $2 }' "$GOLDEN" 2>/dev/null || true)
if [ "${UNDERBOX_BLESS:-}" = 1 ]; then
    { grep -vF "$ADAPTER	" "$GOLDEN" 2>/dev/null || true; printf '%s\t%s\n' "$ADAPTER" "$HASH"; } > "$GOLDEN.new"
    mv "$GOLDEN.new" "$GOLDEN"
    echo "RESULT: BLESSED"
elif [ -z "$WANT" ]; then
    echo "RESULT: SKIP (no golden hash for $ADAPTER; UNDERBOX_BLESS=1 records it)"
elif [ "$WANT" = "$HASH" ]; then
    echo "RESULT: PASS"
else
    echo "want $WANT"; echo "RESULT: FAIL (frame hash changed; look at $OUT/frame.png, bless if intended)"; exit 1
fi
