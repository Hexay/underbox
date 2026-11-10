#!/bin/sh
# Boot Facepunch's dedicated server (unmodified sbox-public) on our libengine2 and summarise.
# usage: tools/boot-server.sh [seconds=60] [sbox-public] [-- extra server args]
# Log: $OUT/server.log (default target/boot). Each stub slot logs once, on its first call.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-60}
SBOX=$(cd -- "${2:-$ROOT/../sbox-public}" && pwd)
shift $(( $# > 2 ? 2 : $# ))
[ "${1:-}" = "--" ] && shift
OUT=${OUT:-$ROOT/target/boot}
mkdir -p "$OUT"

sh "$ROOT/tools/prepare-sbox.sh" "$SBOX"
# Rebuild when missing or when prepare-sbox.sh added EmbeddedRefs after the last build.
if [ ! -f "$SBOX/game/sbox-server.dll" ] || [ "$SBOX/engine/Sandbox.Compiling/EmbeddedRefs" -nt "$SBOX/game/sbox-server.dll" ]; then
    echo "== building sbox-server (~2 min)"
    # The icon isn't in the public repo; an empty property skips it without patching their csproj.
    dotnet build "$SBOX/engine/Launcher/SboxServer/Sbox-Server.csproj" -c Release -m:4 -v q -nologo -clp:ErrorsOnly -p:ApplicationIcon=
fi

cargo build -q --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4
TARGET_DIR=$(cargo metadata -q --format-version 1 --manifest-path "$ROOT/Cargo.toml" --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
NATIVE="$SBOX/game/bin/linuxsteamrt64"
mkdir -p "$NATIVE"
cp "$TARGET_DIR/debug/libengine2.so" "$NATIVE/libengine2.so"
# Managed loads these modules by name for their CreateInterface; they're all engine2, and the
# loader dedupes symlinks by inode so they share its state. TODO(windows): needs forwarder DLLs.
for m in vfx_vulkan filesystem_stdio; do ln -sf libengine2.so "$NATIVE/lib$m.so"; done
# Facepunch ships only SkiaSharp's Windows/macOS natives; match Sandbox.Engine.csproj's version.
# s&box's DLLImportResolver loads natives from its native dir, not runtimes/.
sh "$ROOT/tools/nuget-file.sh" SkiaSharp.NativeAssets.Linux 3.116.0 runtimes/linux-x64/native/libSkiaSharp.so \
    "$NATIVE/libSkiaSharp.so"

cd "$SBOX/game"
timeout -s INT "$SECS" dotnet sbox-server.dll "$@" > "$OUT/server.log" 2>&1 || echo "exit: $?"

echo "== engine2"; grep -E '^engine2:' "$OUT/server.log" | head -n 5 || true
echo "== first stubs called"; grep -E '^\[Warn\] stub:' "$OUT/server.log" | head -n 15 || true
echo "== stub slots hit: $(grep -cE '^\[Warn\] stub:' "$OUT/server.log" || true)"
echo "== errors"; grep -iE 'exception|error|fatal' "$OUT/server.log" | grep -v '^\[Warn\] stub' | head -n 15 || true
echo "== tail"; tail -n 8 "$OUT/server.log"
