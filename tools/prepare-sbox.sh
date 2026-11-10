#!/bin/sh
# Fill in what sbox-public's build expects but the public repo doesn't carry, without Facepunch's
# artifact download (it bundles their closed native engine). Idempotent.
# usage: tools/prepare-sbox.sh <sbox-public>
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SBOX=$1

# Facepunch's bootstrap downloads this into ThirdParty; NuGet's copy (MIT) compiles the same.
sh "$ROOT/tools/nuget-file.sh" System.Speech 10.0.14 lib/net10.0/System.Speech.dll "$SBOX/engine/ThirdParty/System.Speech.dll"

# The game-code compiler embeds .NET's reference assemblies (Sandbox.Compiling.csproj); without
# them every game fails with "Missing Core Assembly". The SDK's own ref pack is the same MIT set.
REFS="$SBOX/engine/Sandbox.Compiling/EmbeddedRefs"
if [ ! -d "$REFS" ]; then
    DOTNET=$(dirname -- "$(readlink -f -- "$(command -v dotnet)")")
    PACK=$(ls -d "$DOTNET"/packs/Microsoft.NETCore.App.Ref/10.* | sort -V | tail -n 1)
    echo "== EmbeddedRefs from $PACK"
    mkdir -p "$REFS"
    cp "$PACK"/ref/net10.0/*.dll "$REFS/"
fi
