#!/bin/sh
# Regenerate the interop description (abi/*.json) and the Rust ABI crate from an sbox-public checkout.
# usage: tools/regen.sh [path/to/sbox-public]   (default: ../sbox-public)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SBOX=$(cd -- "${1:-$ROOT/../sbox-public}" && pwd)
DUMP="dotnet run --project $ROOT/tools/interop-dump -c Release -p:SboxPublic=$SBOX --"
mkdir -p "$ROOT/abi"

echo "== managed bindings (InteropGen, managed side only)"
$DUMP generate "$SBOX/engine" | grep -E 'Error|engine.def' || true

# Facepunch's bootstrap downloads this into ThirdParty; NuGet's copy (MIT) compiles the same.
SPEECH="$SBOX/engine/ThirdParty/System.Speech.dll"
if [ ! -f "$SPEECH" ]; then
    echo "== System.Speech.dll from NuGet"
    TMP=$(mktemp -d)
    curl -sfL https://api.nuget.org/v3-flatcontainer/system.speech/10.0.14/system.speech.10.0.14.nupkg -o "$TMP/pkg.zip"
    unzip -q -j "$TMP/pkg.zip" 'lib/net10.0/System.Speech.dll' -d "$TMP"
    cp "$TMP/System.Speech.dll" "$SPEECH"
    rm -rf "$TMP"
fi

echo "== Sandbox.Engine.dll (background-worthy: ~2 min cold)"
dotnet build "$SBOX/engine/Sandbox.Engine/Sandbox.Engine.csproj" -c Release -m:4 -v q -nologo -clp:ErrorsOnly
DLL="$SBOX/game/bin/managed/Sandbox.Engine.dll"

echo "== abi/engine.json, abi/layouts.json"
$DUMP dump "$SBOX/engine/Definitions/engine.def" "$ROOT/abi/engine.json"
$DUMP layouts "$ROOT/abi/engine.json" "$DLL" "$ROOT/abi/layouts.json"
git -C "$SBOX" rev-parse HEAD > "$ROOT/abi/SBOX_PUBLIC_COMMIT"

echo "== crates/abi/src/generated"
cargo run -q --manifest-path "$ROOT/Cargo.toml" -p abigen -- "$ROOT/abi" "$ROOT/crates/abi/src/generated"
