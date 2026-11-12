#!/bin/sh
# One step of tools/verify.sh: run <cmd...> from the repo root with OUT=<dir>/<name> (client-frame.sh
# writes there, so concurrent frames don't share target/client-frame), its output in
# <dir>/<name>.log, then "<exit status> <seconds>" in <dir>/<name>.rc. Once any step has failed,
# later ones only record "skip". Always exits 0: xargs gives up on a 255.
# usage: tools/verify-step.sh <dir> <name> <cmd...>
set -u
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
DIR=$1 NAME=$2
shift 2
[ -e "$DIR/.failed" ] && { echo "skip 0" > "$DIR/$NAME.rc"; exit 0; }
s=$(date +%s)
rc=0
(cd "$ROOT" && OUT="$DIR/$NAME" "$@") > "$DIR/$NAME.log" 2>&1 || rc=$?
[ $rc -eq 0 ] || : > "$DIR/.failed"
echo "$rc $(($(date +%s) - s))" > "$DIR/$NAME.rc"
