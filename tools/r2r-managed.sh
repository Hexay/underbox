#!/bin/sh
# Precompile sbox-public's managed dlls to ReadyToRun so boots don't JIT the engine and Roslyn.
# usage: tools/r2r-managed.sh <sbox-public>   (boot-server.sh runs it; a no-op when up to date)
# Only dlls newer than the stamp are compiled, so a rebuild of sbox-server is picked up. The runtime
# rejects R2R code whose dependencies changed and JITs instead, so a stale image is slow, not wrong.
set -eu
M=$1/game/bin/managed
STAMP=$M/.underbox-r2r
[ -f "$STAMP" ] && [ -z "$(find "$M" -maxdepth 1 -name '*.dll' -newer "$STAMP" | head -n 1)" ] && exit 0

FW=$(dotnet --list-runtimes | awk '$1 == "Microsoft.NETCore.App" { v = $2; d = $3 } END { gsub(/[][]/, "", d); print d "/" v }')
VER=$(basename -- "$FW")
CG=${XDG_CACHE_HOME:-$HOME/.cache}/underbox/crossgen2-$VER
if [ ! -x "$CG/tools/crossgen2" ]; then
    echo "== crossgen2 $VER from NuGet"
    PKG=microsoft.netcore.app.crossgen2.linux-x64
    TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
    curl -sfL "https://api.nuget.org/v3-flatcontainer/$PKG/$VER/$PKG.$VER.nupkg" -o "$TMP/pkg.zip"
    mkdir -p "$CG"; unzip -q -o "$TMP/pkg.zip" 'tools/*' -d "$CG"; chmod +x "$CG/tools/crossgen2"
fi

NEW=$( [ -f "$STAMP" ] && find "$M" -maxdepth 1 -name '*.dll' -newer "$STAMP" || find "$M" -maxdepth 1 -name '*.dll')
echo "== ReadyToRun: $(echo "$NEW" | wc -l) managed dlls (one-off, ~1.5 min)"
OUTD=$(mktemp -d)
# An input that is already R2R fails to compile and is kept as is.
echo "$NEW" | xargs -P4 -I{} sh -c '"$0/tools/crossgen2" "$1" -o "$2/${1##*/}" -r "$3/*.dll" -r "$4/*.dll" \
    --targetos linux --targetarch x64 -O > /dev/null 2>&1 && mv -f "$2/${1##*/}" "$1" || true' "$CG" {} "$OUTD" "$M" "$FW"
rm -rf "$OUTD"
touch "$STAMP"
