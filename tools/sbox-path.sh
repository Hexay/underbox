#!/bin/sh
# Print the absolute sbox-public checkout: $1 if given, else the sibling of the main underbox
# checkout (resolved through git, so it also works from a worktree under .claude/worktrees).
# usage: SBOX=$(sh tools/sbox-path.sh "${2:-}")
set -eu
if [ -n "${1:-}" ]; then
    (cd -- "$1" && pwd)
else
    ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
    MAIN=$(dirname -- "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")
    (cd -- "$MAIN/../sbox-public" && pwd)
fi
