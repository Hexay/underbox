#!/bin/sh
# Print a game port unique to this worktree (20000-23999), so worktrees can boot servers in parallel.
# usage: PORT=${PORT:-$(sh tools/worktree-port.sh <0-3>)}   (one offset per script that hosts)
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
echo $(( 20000 + $(printf %s "$ROOT" | cksum | cut -d' ' -f1) % 1000 * 4 + ${1:-0} ))
