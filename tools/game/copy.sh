#!/bin/sh
# Sync a read-only game checkout into a scratch copy that the runs may change (tools/game.sh, tools/diff/frame.sh).
# usage: sh tools/game/copy.sh <checkout> <copy>
set -eu
CHECKOUT=$1
COPY=$2
[ -d "$COPY" ] || cp -a --reflink=auto "$CHECKOUT" "$COPY"
rsync -a --delete --exclude=.git "$CHECKOUT/" "$COPY/"
# Assets/ and assets/ (Code/ and code/) are one folder where a game was authored (Windows), two here,
# and only the capitalised one is read.
for d in Assets Code; do
    lower=$(echo "$d" | tr 'A-Z' 'a-z')
    if [ -d "$COPY/$lower" ]; then
        mkdir -p "$COPY/$d"
        rsync -a --ignore-existing "$COPY/$lower/" "$COPY/$d/"
        rm -rf "${COPY:?}/$lower"
    fi
done
