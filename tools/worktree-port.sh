#!/bin/sh
# Borrow a game port for one boot: the first of this worktree's 12 (20000-31999) that nothing
# listens on and no live boot holds. The hold lasts while <owner-pid> lives, so boots in one
# worktree, and in different worktrees, never share a port.
# usage: PORT=${PORT:-$(sh tools/worktree-port.sh $$)}
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
OWNER=${1:?usage: worktree-port.sh <owner-pid>}
BASE=$(( 20000 + $(printf %s "$ROOT" | cksum | cut -d' ' -f1) % 1000 * 12 ))
HOLDS=${UNDERBOX_PORT_HOLDS:-/srv/agents/cache/underbox-ports}
mkdir -p "$HOLDS"

i=0
while [ $i -lt 12 ]; do
    p=$((BASE + i)) hold=$HOLDS/$((BASE + i))
    # mkdir is the atomic claim; the pid file follows it.
    if mkdir "$hold" 2>/dev/null; then
        if [ -z "$(ss -Hluna "sport = :$p" 2>/dev/null)" ]; then
            echo "$OWNER" > "$hold/pid"
            echo "$p"
            exit 0
        fi
        rmdir "$hold"
    else
        pid=$(cat "$hold/pid" 2>/dev/null || true)
        # An empty pid is a claim being made right now, unless it is over a minute old.
        if { [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; } || { [ -z "$pid" ] && [ -n "$(find "$hold" -maxdepth 0 -mmin +1)" ]; }; then
            rm -rf "${hold:?}"
            continue
        fi
    fi
    i=$((i + 1))
done
echo "worktree-port: ports $BASE-$((BASE + 11)) all held or in use" >&2
exit 1
