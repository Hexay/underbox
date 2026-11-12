#!/bin/sh
# Build the GPU capture layer (tools/diff/gpu/) into $UNDERBOX_CACHE/underbox-gpu-layer/<source hash>/ and
# print that directory: it holds the layer and its manifest, for VK_ADD_LAYER_PATH. See docs/differential-gpu.md.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/cache-dir.sh"
SRC=$ROOT/tools/diff/gpu
HASH=$(cat "$SRC"/*.h "$SRC"/*.cpp "$0" | cksum | cut -d' ' -f1)
OUT=$UNDERBOX_CACHE/underbox-gpu-layer/$HASH
LIB=libVkLayer_underbox_gpu.so
[ -f "$OUT/$LIB" ] && [ -f "$OUT/underbox_gpu.json" ] && { echo "$OUT"; exit 0; }
mkdir -p "$OUT/obj"
ls "$SRC"/*.cpp | xargs -P "${JOBS:-4}" -I{} sh -c \
    'o="$1/obj/$(basename -- "$2" .cpp).o"; c++ -std=c++20 -O2 -g -fPIC -fvisibility=hidden -Wall -Wno-missing-field-initializers -c "$2" -o "$o" > "$o.log" 2>&1' \
    _ "$OUT" {} || { cat "$OUT"/obj/*.log | grep -E -A3 'error' | head -n 40 >&2; exit 1; }
c++ -shared -o "$OUT/$LIB.partial" "$OUT"/obj/*.o
mv "$OUT/$LIB.partial" "$OUT/$LIB"
printf '%s\n' '{"file_format_version": "1.2.0", "layer": {"name": "VK_LAYER_UNDERBOX_gpu_capture", "type": "GLOBAL",' \
    "\"library_path\": \"./$LIB\", \"api_version\": \"1.4.0\", \"implementation_version\": \"1\"," \
    '"description": "underbox GPU frame capture (docs/differential-gpu.md)"}}' > "$OUT/underbox_gpu.json"
echo "$OUT"
