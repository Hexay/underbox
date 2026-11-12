#!/bin/sh
# Build our libengine2 and install it (plus the natives managed loads by name) into a game dir.
# usage: tools/install-engine2.sh <game-dir>   (this worktree's copy, from tools/game-dir.sh)
# UNDERBOX_ENGINE2=<libengine2.so> installs that build instead of building debug (e.g. dist/, docs/release.md).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
GAME=$1

SO=${UNDERBOX_ENGINE2:-}
if [ -z "$SO" ]; then
    # see docs/dev-loop.md (incremental)
    env -u CARGO_INCREMENTAL cargo build -q --manifest-path "$ROOT/Cargo.toml" -p engine2 -j4
    SO="${CARGO_TARGET_DIR:-$ROOT/target}/debug/libengine2.so"
fi
NATIVE="$GAME/bin/linuxsteamrt64"
mkdir -p "$NATIVE"
# Rename, not overwrite: a process still mapping the old file would crash on an in-place rewrite.
cp --reflink=auto "$SO" "$NATIVE/.libengine2.so.$$"
mv -f "$NATIVE/.libengine2.so.$$" "$NATIVE/libengine2.so"
# Managed loads these modules by name for their CreateInterface; they're all engine2, and the
# loader dedupes symlinks by inode so they share its state. TODO(windows): needs forwarder DLLs.
for m in vfx_vulkan filesystem_stdio; do ln -sf libengine2.so "$NATIVE/lib$m.so"; done
# Managed DllImports a few flat Steam calls from steam_api64, which its resolver maps to
# libsteam_api.so on Linux; engine2 answers them offline (steam/flat_api.rs). A real one is kept.
[ -f "$NATIVE/libsteam_api.so" ] && [ ! -L "$NATIVE/libsteam_api.so" ] || ln -sf libengine2.so "$NATIVE/libsteam_api.so"
rm -f "$NATIVE/libsteam_api64.so"
# Facepunch ships only SkiaSharp's Windows/macOS natives; match Sandbox.Engine.csproj's version.
# s&box's DLLImportResolver loads natives from its native dir, not runtimes/.
sh "$ROOT/tools/nuget-file.sh" SkiaSharp.NativeAssets.Linux 3.116.0 runtimes/linux-x64/native/libSkiaSharp.so \
    "$NATIVE/libSkiaSharp.so"
# The client shapes text with HarfBuzzSharp (SkiaSharp.HarfBuzz 3.116.0 pulls 8.3.0).
sh "$ROOT/tools/nuget-file.sh" HarfBuzzSharp.NativeAssets.Linux 8.3.0 runtimes/linux-x64/native/libHarfBuzzSharp.so \
    "$NATIVE/libHarfBuzzSharp.so"
# s&box's DLLImportResolver can miss an assembly under load, and .NET's fallback then probes only
# the game dir and bin/managed. Symlinks share the inode, so the loader never maps a second copy.
if [ -d "$GAME/bin/managed" ]; then
    for f in libSkiaSharp libHarfBuzzSharp; do ln -sfn "../linuxsteamrt64/$f.so" "$GAME/bin/managed/$f.so"; done
    ln -sfn ../linuxsteamrt64/libsteam_api.so "$GAME/bin/managed/libsteam_api64.so"
fi

for f in libengine2 libvfx_vulkan libfilesystem_stdio libsteam_api libSkiaSharp libHarfBuzzSharp; do
    [ "$(od -An -tx1 -N4 "$NATIVE/$f.so" 2>/dev/null | tr -d ' ')" = 7f454c46 ] ||
        { echo "install-engine2: $NATIVE/$f.so is missing or not ELF; refusing to boot" >&2; exit 3; }
done
[ -z "${UNDERBOX_TAP_DIR:-}" ] || sh "$ROOT/tools/diff/tap-install.sh" "$NATIVE"
