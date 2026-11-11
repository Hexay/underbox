#!/bin/sh
# Download the prebuilt engine artifacts that sbox-public's Setup.sh restores, for the newest commit
# at or before <commit> that has a manifest, into $REAL_ENGINE2_CACHE/<hit>/ (tools/diff/env.sh).
# usage: REAL_ENGINE2_HOST=<base url> tools/diff/fetch.sh [commit=abi/SBOX_PUBLIC_COMMIT] [path-prefix...]
#   REAL_ENGINE2_HOST: the base URL in sbox-public's DownloadPublicArtifacts.cs (not repeated here).
#   default prefixes: game/bin/linuxsteamrt64/ game/core/
# Prints the artifact root. Never point the cache inside the repo: see docs/differential.md.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/diff/env.sh"
SBOX=$(sh "$ROOT/tools/sbox-path.sh" "")
BASE=${REAL_ENGINE2_HOST:?set REAL_ENGINE2_HOST to the base URL in sbox-public DownloadPublicArtifacts.cs}
WANT=${1:-$(cat "$ROOT/abi/SBOX_PUBLIC_COMMIT")}
[ $# -gt 0 ] && shift
[ $# -eq 0 ] && set -- game/bin/linuxsteamrt64/ game/core/

HIT=
for c in $(git -C "$SBOX" rev-list "$WANT" --max-count=128); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/manifests/$c.json")
    [ "$code" = 200 ] && { HIT=$c; break; }
done
[ -n "$HIT" ] || { echo "fetch: no manifest within 128 commits of $WANT" >&2; exit 1; }
[ "$HIT" = "$(git -C "$SBOX" rev-parse "$WANT")" ] || echo "fetch: nearest manifest is $HIT" >&2

D=$REAL_ENGINE2_CACHE/$HIT
mkdir -p "$D/meta" "$D/blobs" "$D/game"
[ -s "$D/meta/manifest.json" ] || curl -fsS -o "$D/meta/manifest.json" "$BASE/manifests/$HIT.json"

prefixes=$(printf '%s\n' "$@" | jq -R . | jq -sc .)
# Reject anything that could escape $D: absolute, backslashed or dotted paths.
jq -r --argjson p "$prefixes" '.files[] | select(.path as $f | any($p[]; . as $x | $f | startswith($x)))
    | select(.path | test("^game/[A-Za-z0-9_./ +-]+$") and (test("\\.\\./|/\\.\\.$") | not))
    | "\(.sha256)\t\(.path)"' "$D/meta/manifest.json" > "$D/meta/wanted.tsv"

# Blobs are content-addressed; aliases (libfoo.so, libfoo.so.1) become hard links to one blob.
cut -f1 "$D/meta/wanted.tsv" | sort -u | while read -r h; do
    [ -f "$D/blobs/$h" ] || echo "$h"
done | xargs -r -P 8 -I{} sh -c '
    t=$1/blobs/.$2.part
    curl -fsS --retry 3 -o "$t" "$3/artifacts/$2" &&
    [ "$(sha256sum "$t" | cut -d" " -f1)" = "$2" ] && mv "$t" "$1/blobs/$2" ||
    { rm -f "$t"; echo "fetch: $2 failed" >&2; exit 1; }' _ "$D" {} "$BASE"

while IFS="$(printf '\t')" read -r h p; do
    mkdir -p "$D/$(dirname -- "$p")"
    ln -f "$D/blobs/$h" "$D/$p"
done < "$D/meta/wanted.tsv"
chmod +x "$D/game/bin/linuxsteamrt64/resourcecompiler" 2>/dev/null || true
echo "fetch: $(wc -l < "$D/meta/wanted.tsv") files, $(du -sh "$D/blobs" | cut -f1) of blobs" >&2
echo "$D"
