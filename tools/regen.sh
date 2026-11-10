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

sh "$ROOT/tools/prepare-sbox.sh" "$SBOX"

echo "== Sandbox.Engine.dll (background-worthy: ~2 min cold)"
dotnet build "$SBOX/engine/Sandbox.Engine/Sandbox.Engine.csproj" -c Release -m:4 -v q -nologo -clp:ErrorsOnly
DLL="$SBOX/game/bin/managed/Sandbox.Engine.dll"

echo "== abi/engine.json, abi/layouts.json"
$DUMP dump "$SBOX/engine/Definitions/engine.def" "$ROOT/abi/engine.json"
$DUMP layouts "$ROOT/abi/engine.json" "$DLL" "$ROOT/abi/layouts.json"
git -C "$SBOX" rev-parse HEAD > "$ROOT/abi/SBOX_PUBLIC_COMMIT"
# GameResource extensions engine code declares; the resource compiler compiles these (crates/assets).
grep -rhoE '\[AssetType\([^]]*Extension *= *"[a-z0-9_]+"' "$SBOX/engine" --include=*.cs \
    | sed -E 's/.*Extension *= *"([a-z0-9_]+)".*/\1/' | sort -u > "$ROOT/abi/game_resource_extensions.txt"

echo "== crates/abi/src/generated"
cargo run -q --manifest-path "$ROOT/Cargo.toml" -p abigen -- "$ROOT/abi" "$ROOT/crates/abi/src/generated"
