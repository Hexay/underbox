#!/bin/sh
# Run a command holding the shared sbox-public boot lock (guards the managed build in sbox-public/game).
# usage: [BOOT_LOCK_WAIT=s] tools/boot-lock.sh <cmd...>
# Re-entrant: a nested call (UNDERBOX_BOOT_LOCKED set, or an ancestor process holds the lock) runs cmd
# directly. Waits at most BOOT_LOCK_WAIT (default 600) s, then names the holder and exits 75.
set -eu
LOCK=${UNDERBOX_BOOT_LOCK:-/srv/agents/cache/underbox-boot.lock}
WAIT=${BOOT_LOCK_WAIT:-600}
[ -n "${UNDERBOX_BOOT_LOCKED:-}" ] && exec "$@"
[ -e "$LOCK" ] || : >> "$LOCK"

# Pid that took the lock, from /proc/locks (matched by inode; "->" lines are waiters).
holder() {
    ino=$(stat -c %i "$LOCK")
    awk -v ino="$ino" '$2 == "FLOCK" { split($6, a, ":"); if (a[3] == ino) { print $5; exit } }' /proc/locks
}
is_ancestor() {
    p=$$
    while [ "$p" -gt 1 ]; do
        [ "$p" = "$1" ] && return 0
        # comm (field 2) may hold spaces; ppid is the 2nd field after its closing paren.
        p=$(awk '{ sub(/.*\) /, ""); print $2 }' "/proc/$p/stat" 2>/dev/null) || return 1
    done
    return 1
}
describe() {
    echo "  pid $1: $(tr '\0' ' ' < "/proc/$1/cmdline" 2>/dev/null || echo '(exited)')"
}

# `flock <lock> sh tools/match.sh` must not wait on itself (that deadlocked for 5 h).
h=$(holder)
if [ -n "$h" ] && is_ancestor "$h"; then UNDERBOX_BOOT_LOCKED=1 exec "$@"; fi

# -o: lingering children (an MSBuild node, a server) must not inherit and keep holding the lock.
status=0
UNDERBOX_BOOT_LOCKED=1 flock -o -E 75 -w "$WAIT" "$LOCK" "$@" || status=$?
if [ $status -eq 75 ]; then
    {
        echo "boot-lock: gave up after ${WAIT}s waiting for $LOCK"
        h=$(holder)
        echo "held by:"; [ -n "$h" ] && describe "$h"
        # A holder that exited means a child inherited the fd; these still have the file open.
        echo "open in:"
        find /proc/[0-9]*/fd -maxdepth 1 -lname "$LOCK" 2>/dev/null | cut -d/ -f3 | sort -u |
            while read -r p; do describe "$p"; done
    } >&2
fi
exit $status
