#!/bin/sh
# Bring the shared sbox-public managed build up to date, then print a private copy of sbox-public/game
# for one boot of this worktree. Boots run there, so they never share bin/linuxsteamrt64/libengine2.so.
# usage: GAME=$(UNDERBOX_LEASE=<boot pid> sh tools/game-dir.sh <sbox-public> [server|client])
# Copies are btrfs reflinks (about 0.5 s, no extra disk), versioned as <worktree>-<hash>.vXXXXXX.
# A copy in use is never changed or deleted: see docs/dev-loop.md (game dirs).
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SYNC_TO=
[ "$1" = --sync ] && { SYNC_TO=$2; shift 2; }
SBOX=$1
LAUNCHER=${2:-server}
case $LAUNCHER in
    server) DLL=sbox-server.dll; PROJ=SboxServer/Sbox-Server.csproj ;;
    client) DLL=sbox.dll; PROJ=Sbox/Sbox.csproj ;;
    *) echo "game-dir.sh: unknown launcher '$LAUNCHER'" >&2; exit 2 ;;
esac
GAMES=${UNDERBOX_GAME_DIRS:-/srv/agents/cache/underbox-game}
BASE=$GAMES/$(basename -- "$ROOT")-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
CUR=$BASE.current
# Exported: the flock re-exec below changes $PPID.
export UNDERBOX_LEASE=${UNDERBOX_LEASE:-$PPID}
LEASE=$UNDERBOX_LEASE
# A stamp, not the dll's mtime: an up-to-date build copies the dll back with its old mtime.
# It holds the sbox-public commit built, so checking out another commit rebuilds.
BUILT=$SBOX/game/.underbox-$LAUNCHER-built
REFS=$SBOX/engine/Sandbox.Compiling/EmbeddedRefs
HEAD=$(git -C "$SBOX" rev-parse HEAD)

build_stale() {
    [ ! -f "$SBOX/game/$DLL" ] || [ ! -d "$REFS" ] || [ "$REFS" -nt "$BUILT" ] ||
        [ "$(cat "$BUILT" 2>/dev/null)" != "$HEAD" ]
}
# Managed dlls and the build/R2R stamps mark a changed build; natives are installed per copy anyway.
copy_stale() {
    [ ! -f "$1/.underbox-synced" ] ||
        [ -n "$(find "$SBOX/game" -maxdepth 3 \( -name '*.dll' -o -name '.underbox-*' \) -newer "$1/.underbox-synced" | head -n 1)" ]
}
fill() { find "$1" -mindepth 1 -maxdepth 1 ! -name logs ! -name '.underbox-lease.*' -exec cp -a --reflink=auto -t "$2" {} +; }

if [ -n "$SYNC_TO" ]; then
    # Under the boot lock: nobody rebuilds sbox-public/game while it is copied.
    sh "$ROOT/tools/prepare-sbox.sh" "$SBOX"
    if build_stale; then
        echo "== building $PROJ (~2 min)"
        # The icon isn't in the public repo; an empty property skips it without patching their csproj.
        dotnet build "$SBOX/engine/Launcher/$PROJ" -c Release -m:4 -nodeReuse:false -v q -nologo -clp:ErrorsOnly -p:ApplicationIcon=
        echo "$HEAD" > "$BUILT"
    fi
    sh "$ROOT/tools/r2r-managed.sh" "$SBOX"
    echo "== copying sbox-public/game to $SYNC_TO"
    # Not logs/: other boots grow it by GBs, and a copy would pin those extents.
    fill "$SBOX/game" "$SYNC_TO"
    touch "$SYNC_TO/.underbox-synced"
    exit 0
fi

mkdir -p "$GAMES"
[ -n "${UNDERBOX_GAME_LOCKED:-}" ] || UNDERBOX_GAME_LOCKED=1 exec flock -o -w 600 "$BASE.lock" sh "$0" "$@"
exec 3>&1 1>&2

# In use: a live boot other than ours holds a lease (taken here, before its build), or a process runs in it.
in_use() {
    for l in "$1"/.underbox-lease.*; do
        p=${l##*.}
        [ -e "$l" ] && [ "$p" != "$LEASE" ] && kill -0 "$p" 2>/dev/null && return 0
    done
    [ -n "$(find /proc -mindepth 2 -maxdepth 2 -name cwd \( -lname "$1" -o -lname "$1/*" \) -print -quit 2>/dev/null)" ]
}
fresh() { mktemp -d "$BASE.vXXXXXX"; }

V=$(readlink "$CUR" 2>/dev/null) && V=$GAMES/$V && [ -d "$V" ] || V=
if [ -z "$V" ] || build_stale || copy_stale "$V"; then
    NEW=$(fresh)
    sh "$ROOT/tools/boot-lock.sh" sh "$0" --sync "$NEW" "$SBOX" "$LAUNCHER"
    V=$NEW
elif in_use "$V"; then
    # Another boot runs from it: re-pointing its natives or managed dlls would break that boot.
    NEW=$(fresh)
    fill "$V" "$NEW"
    V=$NEW
fi
mkdir -p "$V/logs"
: > "$V/.underbox-lease.$LEASE"
ln -sfn "$(basename -- "$V")" "$CUR"

# Older layouts ($BASE itself, .new/.old) count as versions too.
for d in "$BASE" "$BASE".v* "$BASE.new" "$BASE.old"; do
    [ ! -L "$d" ] && [ -d "$d" ] && [ "$d" != "$V" ] || continue
    in_use "$d" || rm -rf "$d"
done
for l in "$V"/.underbox-lease.*; do kill -0 "${l##*.}" 2>/dev/null || rm -f "$l"; done
echo "$V" >&3
