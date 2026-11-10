#!/bin/sh
# Bring the shared sbox-public managed build up to date, then print this worktree's private copy of
# sbox-public/game, re-copied when the shared build changed. Boots run there, so worktrees don't share
# bin/linuxsteamrt64/libengine2.so and can boot in parallel. Takes the boot lock only when there is
# shared work to do (a build, ReadyToRun, a re-copy); an up-to-date copy needs no lock.
# usage: GAME=$(sh tools/game-dir.sh <sbox-public>)   (progress goes to stderr)
# The copy is a btrfs reflink: about 1 s and no extra disk. Delete it any time; the next boot remakes it.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
SBOX=$1
GAMES=${UNDERBOX_GAME_DIRS:-/srv/agents/cache/underbox-game}
DST=$GAMES/$(basename -- "$ROOT")-$(printf %s "$ROOT" | cksum | cut -d' ' -f1)
SYNCED=$DST/.underbox-synced
# A stamp, not the dll's mtime: an up-to-date build copies the dll back with its old mtime.
BUILT=$SBOX/game/.underbox-server-built
REFS=$SBOX/engine/Sandbox.Compiling/EmbeddedRefs

server_stale() { [ ! -f "$SBOX/game/sbox-server.dll" ] || [ ! -d "$REFS" ] || [ "$REFS" -nt "$BUILT" ]; }
# Managed dlls and the build/R2R stamps mark a changed build; natives are installed per copy anyway.
copy_stale() {
    [ ! -f "$SYNCED" ] || [ -n "$(find "$SBOX/game" -maxdepth 3 \( -name '*.dll' -o -name '.underbox-*' \) -newer "$SYNCED" | head -n 1)" ]
}

if [ -z "${UNDERBOX_BOOT_LOCKED:-}" ]; then
    # The copy was taken under the lock, so it is consistent even while another worktree rebuilds.
    if ! server_stale && ! copy_stale; then echo "$DST"; exit 0; fi
    exec sh "$ROOT/tools/boot-lock.sh" sh "$0" "$@"
fi
exec 3>&1 1>&2

sh "$ROOT/tools/prepare-sbox.sh" "$SBOX"
if server_stale; then
    echo "== building sbox-server (~2 min)"
    # The icon isn't in the public repo; an empty property skips it without patching their csproj.
    dotnet build "$SBOX/engine/Launcher/SboxServer/Sbox-Server.csproj" -c Release -m:4 -nodeReuse:false -v q -nologo -clp:ErrorsOnly -p:ApplicationIcon=
    touch "$BUILT"
fi
sh "$ROOT/tools/r2r-managed.sh" "$SBOX"

if copy_stale; then
    echo "== copying sbox-public/game to $DST"
    mkdir -p "$GAMES"
    rm -rf "$DST.new" "$DST.old"
    mkdir -p "$DST.new/logs"
    # Not logs/: other boots grow it by GBs, and a copy would pin those extents.
    find "$SBOX/game" -mindepth 1 -maxdepth 1 ! -name logs -exec cp -a --reflink=auto -t "$DST.new" {} +
    touch "$DST.new/.underbox-synced"
    [ ! -d "$DST" ] || mv "$DST" "$DST.old"
    mv "$DST.new" "$DST"
    rm -rf "$DST.old"
fi
echo "$DST" >&3
