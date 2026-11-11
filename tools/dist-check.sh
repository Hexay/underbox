#!/bin/sh
# Check a libengine2's dynamic dependencies against the release allowlist (docs/release.md).
# usage: tools/dist-check.sh <libengine2.so>   (DIST_GLIBC_MAX=2.35 also caps the glibc symbol versions)
# Prints `needed:`, `glibc:`, `glibcxx:`, `openssl:` lines (tools/dist.sh puts them in the manifest).
# Exits 1 on an unexpected DT_NEEDED, an undefined snd_* symbol, or a glibc version over the cap.
set -eu
SO=$1
ALLOWED='libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 librt.so.1 ld-linux-x86-64.so.2 libgcc_s.so.1 libstdc++.so.6 libcrypto.so.3 libssl.so.3'
fail=0

needed=$(readelf -d "$SO" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort)
for lib in $needed; do
    case " $ALLOWED " in
        *" $lib "*) ;;
        *) echo "FAIL unexpected dynamic dependency: $lib"; fail=1 ;;
    esac
done

# Highest version of each versioned dependency; `sort -V` orders 2.9 before 2.10.
maxver() { objdump -T "$SO" | grep -oE "\($1_[0-9.]+\)" | tr -d '()' | sed "s/^$1_//" | sort -uV | tail -n 1; }
glibc=$(maxver GLIBC)
if [ -n "${DIST_GLIBC_MAX:-}" ] && [ "$(printf '%s\n%s\n' "$glibc" "$DIST_GLIBC_MAX" | sort -V | tail -n 1)" != "$DIST_GLIBC_MAX" ]; then
    echo "FAIL glibc $glibc is over the $DIST_GLIBC_MAX baseline; the newest symbols:"
    objdump -T "$SO" | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^\(GLIBC_/) { v = $i; gsub(/[()]|GLIBC_/, "", v); print v, $NF } }' \
        | sort -uV | tail -n 8
    fail=1
fi

# alsa_dl must define every snd_* the alsa crate calls; an undefined one fails to load under BIND_NOW.
snd=$(nm -D --undefined-only "$SO" | awk '$2 ~ /^snd_/ { print $2 }')
if [ -n "$snd" ]; then
    echo "FAIL undefined ALSA symbols (add them to crates/audio/src/alsa_dl/symbols.rs):"; echo "$snd" | head -n 20
    fail=1
fi

echo "needed: $(echo $needed)"
echo "glibc: $glibc"
echo "glibcxx: $(maxver GLIBCXX)"
echo "openssl: $(maxver OPENSSL)"
if [ $fail -eq 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL"; fi
exit $fail
