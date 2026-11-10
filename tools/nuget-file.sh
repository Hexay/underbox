#!/bin/sh
# Copy one file out of a NuGet package to DEST, unless DEST exists.
# usage: tools/nuget-file.sh <package> <version> <path in package> <dest>
set -eu
PKG=$(echo "$1" | tr 'A-Z' 'a-z'); VER=$2; FILE=$3; DEST=$4
[ -f "$DEST" ] && exit 0
echo "== $FILE from NuGet $1 $VER"
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
curl -sfL "https://api.nuget.org/v3-flatcontainer/$PKG/$VER/$PKG.$VER.nupkg" -o "$TMP/pkg.zip"
unzip -q -j "$TMP/pkg.zip" "$FILE" -d "$TMP"
mkdir -p "$(dirname -- "$DEST")"
cp "$TMP/$(basename -- "$FILE")" "$DEST"
