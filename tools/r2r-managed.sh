#!/bin/sh
# Precompile sbox-public's managed dlls to ReadyToRun so boots don't JIT the engine and Roslyn.
# usage: tools/r2r-managed.sh <sbox-public>   (game-dir.sh runs it; a no-op when up to date)
# Which dlls are pending is read from their headers (tools/r2r-pending.py), not mtimes. The runtime
# rejects R2R code whose dependencies changed and JITs instead, so a stale image is slow, not wrong.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
M=$1/game/bin/managed
NEW=$(python3 -I "$ROOT/tools/r2r-pending.py" "$M")
[ -n "$NEW" ] || exit 0

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

echo "== ReadyToRun: $(echo "$NEW" | wc -l) managed dlls (~25 s for all)"
OUTD=$(mktemp -d)
echo "$NEW" | xargs -P4 -I{} sh -c '"$0/tools/crossgen2" "$1" -o "$2/${1##*/}" -r "$3/*.dll" -r "$4/*.dll" \
    --targetos linux --targetarch x64 -O > "$2/${1##*/}.log" 2>&1 && mv -f "$2/${1##*/}" "$1" ||
    echo "r2r-managed: ${1##*/} stays IL: $(tail -n 1 "$2/${1##*/}.log")"' "$CG" {} "$OUTD" "$M" "$FW"
rm -rf "$OUTD"
python3 -I "$ROOT/tools/r2r-pending.py" --failed "$M"
