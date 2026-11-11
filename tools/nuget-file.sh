#!/bin/sh
# Install one file from a NuGet package at DEST, unless DEST already matches. The file is cached once per
# machine; both the cache and DEST are written by rename, so no reader ever sees a partial file.
# usage: tools/nuget-file.sh <package> <version> <path in package> <dest>
set -eu
PKG=$(echo "$1" | tr 'A-Z' 'a-z'); VER=$2; FILE=$3; DEST=$4
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/underbox/nuget/$PKG.$VER/$FILE
if [ ! -s "$CACHE" ]; then
    echo "== $FILE from NuGet $1 $VER"
    TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
    curl -sfL "https://api.nuget.org/v3-flatcontainer/$PKG/$VER/$PKG.$VER.nupkg" -o "$TMP/pkg.zip"
    unzip -q -j "$TMP/pkg.zip" "$FILE" -d "$TMP"
    mkdir -p "$(dirname -- "$CACHE")"
    cp "$TMP/$(basename -- "$FILE")" "$CACHE.$$"
    mv -f "$CACHE.$$" "$CACHE"
fi
cmp -s "$CACHE" "$DEST" && exit 0
mkdir -p "$(dirname -- "$DEST")"
cp --reflink=auto "$CACHE" "$DEST.$$"
mv -f "$DEST.$$" "$DEST"
