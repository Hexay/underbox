# Sourced by tools/boot-server.sh and tools/boot-client.sh for every run, either engine: managed code
# fetches packages and calls sbox.game's web API, and no run may reach it (docs/differential.md,
# "Rules"). HTTP(S) goes to a closed port; loopback stays direct (game.sh's join serves its local
# package manifest there).
export HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9
export http_proxy=$HTTP_PROXY https_proxy=$HTTPS_PROXY all_proxy=$ALL_PROXY
case ",${NO_PROXY-}," in *,127.0.0.1,*) ;; *) NO_PROXY="127.0.0.1,localhost${NO_PROXY:+,$NO_PROXY}" ;; esac
export NO_PROXY no_proxy="$NO_PROXY"

# from_local_project <game-dir-relative file>: with UNDERBOX_LOCAL_PROJECT (game.sh's join), the
# loopback manifest's cache and the project's own resources, which the client caches as
# download/assets/<path>.<crc>.<ext>.
from_local_project() {
    p=${UNDERBOX_LOCAL_PROJECT:-}
    [ -n "$p" ] || return 1
    case $1 in
        .source2/cache/services/manifest.*) return 0 ;;
        download/assets/*) ;;
        *) return 1 ;;
    esac
    rel=$(printf '%s\n' "${1#download/assets/}" | sed -E 's/\.[0-9a-f]{16}(\.[^./]+)$/\1/; s/_c$//')
    [ -n "$(find "$p/Assets" -ipath "$p/Assets/$rel*" -print -quit 2>/dev/null)" ]
}

# no_downloads <game-dir> [seeded files, as `find .` lists them]: after a run. Anything else in
# download/, or a package record or manifest in the services cache, was fetched: print it loudly,
# delete it, return 1.
no_downloads() {
    found=$(cd "$1" && find download .source2/cache/services -type f 2>/dev/null | while read -r f; do
        case $f in .source2/*/package_types.json) continue ;; .source2/*/package.*|.source2/*/manifest.*|download/*) ;; *) continue ;; esac
        printf '%s\n' "${2:-}" | grep -qxF "./$f" && continue
        from_local_project "$f" || printf '%s\n' "$f"
    done)
    [ -n "$found" ] || return 0
    echo "FAIL: the game downloaded $(printf '%s\n' "$found" | wc -l) files into $1; no run may reach sbox.game (docs/differential.md). Deleted:" | tee /dev/stderr
    printf '%s\n' "$found" | head -n 10
    (cd "$1" && printf '%s\n' "$found" | while read -r f; do rm -f "$f"; done)
    return 1
}
