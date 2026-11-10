#!/bin/sh
# Check out the submodules if any is missing (fresh worktrees start without them). A few ms when present.
# usage: sh tools/submodules.sh   (tools/test.sh and tools/boot-server.sh run it)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
for p in $(git config -f "$ROOT/.gitmodules" --get-regexp '\.path$' | cut -d' ' -f2); do
    [ -e "$ROOT/$p/.git" ] && continue
    echo "== git submodule update --init" >&2
    git -C "$ROOT" submodule update --init >&2
    break
done
