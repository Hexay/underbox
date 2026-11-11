#!/bin/sh
# Fetch Microsoft's official DXC release (libdxcompiler / dxcompiler.dll) into $UNDERBOX_DXC_DIR
# (default $UNDERBOX_CACHE/dxc, tools/cache-dir.sh), pinned by version and sha256. Prints the library directory.
# usage: tools/fetch-dxc.sh            crates/shaders finds it through $UNDERBOX_DXC_DIR or that default
set -eu
TAG=v1.9.2609
. "$(dirname -- "$0")/cache-dir.sh"
ROOT=$UNDERBOX_DXC_DIR
DEST="$ROOT/$TAG"
BASE="https://github.com/microsoft/DirectXShaderCompiler/releases/download/$TAG"
case "$(uname -s)" in
  Linux) FILE=linux_dxc_2026_09_28.x86_x64.tar.gz
         SHA=96faadc7f5c282d2ffda49804beb4c3ee38127bc252b723234e3c5cdf7aa39a1
         LIB="$DEST/lib/libdxcompiler.so" ;;
  MINGW*|MSYS*|CYGWIN*) FILE=dxc_2026_09_29.zip
         SHA=ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1
         LIB="$DEST/bin/x64/dxcompiler.dll" ;;
  *) echo "fetch-dxc: no pinned DXC build for $(uname -s)" >&2; exit 1 ;;
esac
if [ ! -f "$LIB" ]; then
  TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
  curl -sfL "$BASE/$FILE" -o "$TMP/$FILE"
  echo "$SHA  $TMP/$FILE" | sha256sum -c - >/dev/null
  mkdir -p "$DEST"
  case "$FILE" in
    *.zip) unzip -q -o "$TMP/$FILE" -d "$DEST" ;;
    *) tar -xzf "$TMP/$FILE" -C "$DEST" ;;
  esac
  [ -f "$LIB" ] || { echo "fetch-dxc: $LIB missing after extract" >&2; exit 1; }
fi
dirname -- "$LIB"
