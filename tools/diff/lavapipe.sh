#!/bin/sh
# Build a user-local lavapipe from a pinned Mesa commit and print its ICD manifest, for real engine2
# clients (tools/diff/real-env.sh picks it up). No root: build-only headers and tools come from
# `apt-get download` .debs (SHA512 checked) unpacked into a sysroot; runtime libs are the system's.
# Why: Mesa 26.0's llvmpipe reads an SSBO descriptor through lane 0 when the exec mask is empty, which
# segfaults real's batched UI shader (docs/differential.md, "Real's UI on lavapipe").
# Out: $UNDERBOX_CACHE/mesa-lvp/{src,sysroot,build,install}; about 0.7 GB, 10 min with -j6.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/../.." && pwd)
. "$ROOT/tools/cache-dir.sh"
MESA_COMMIT=${MESA_COMMIT:-34c83ff13b3b1d914280c8902d170b60f1e3b161}
M=$UNDERBOX_CACHE/mesa-lvp
ICD=$M/install/share/vulkan/icd.d/lvp_icd.x86_64.json
[ -f "$ICD" ] && [ "$(cat "$M/install/.commit" 2>/dev/null)" = "$MESA_COMMIT" ] && { echo "$ICD"; exit 0; }
mkdir -p "$M/debs" "$M/sysroot"
SR=$M/sysroot

if [ ! -d "$M/src/.git" ] || [ "$(git -C "$M/src" rev-parse HEAD)" != "$MESA_COMMIT" ]; then
    rm -rf "$M/src" "$M/build" && git init -q "$M/src"
    git -C "$M/src" fetch -q --depth 1 https://gitlab.freedesktop.org/mesa/mesa.git "$MESA_COMMIT"
    git -C "$M/src" checkout -q FETCH_HEAD
fi

# Some cloud mirrors stall mid-transfer: fetch from the main archive instead.
apt-get download --print-uris libxcb-dri3-dev libxcb-present-dev libxcb-shm0-dev libxcb-randr0-dev \
    libx11-xcb-dev libxshmfence-dev libxcb-xfixes0-dev libxcb-sync-dev libxcb-keysyms1-dev libxrandr-dev \
    libxfixes-dev libdrm-dev libxcb1-dev libx11-dev libxext-dev libxrender-dev x11proto-dev libxau-dev \
    libxdmcp-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xkb-dev libxcb-glx0-dev \
    bison flex libfl-dev m4 glslang-tools spirv-tools |
    tr -d "'" | sed 's#http://[^/]*/ubuntu/#http://archive.ubuntu.com/ubuntu/#' > "$M/uris.txt"
while read -r uri name size sum; do
    f=$M/debs/$name
    if [ "$(sha512sum "$f" 2>/dev/null | cut -d' ' -f1)" != "${sum#SHA512:}" ]; then
        curl -sf -m 120 --connect-timeout 10 --retry 3 -o "$f" "$uri"
        [ "$(sha512sum "$f" | cut -d' ' -f1)" = "${sum#SHA512:}" ] || { echo "lavapipe: bad hash for $name" >&2; exit 1; }
    fi
    dpkg -x "$f" "$SR"
done < "$M/uris.txt"

[ -x "$M/venv/bin/python" ] || uv venv -q "$M/venv"
VIRTUAL_ENV=$M/venv uv pip install -q mako pyyaml packaging
export PATH=$M/venv/bin:$SR/usr/bin:$PATH BISON_PKGDATADIR=$SR/usr/share/bison M4=$SR/usr/bin/m4
export PKG_CONFIG_PATH=$SR/usr/lib/x86_64-linux-gnu/pkgconfig:$SR/usr/share/pkgconfig
# --define-prefix misreads multiarch .pc dirs, so the include dirs are given directly too.
export C_INCLUDE_PATH=$SR/usr/include:$SR/usr/include/libdrm CPLUS_INCLUDE_PATH=$SR/usr/include:$SR/usr/include/libdrm
export LIBRARY_PATH=$SR/usr/lib/x86_64-linux-gnu LD_LIBRARY_PATH=$SR/usr/lib/x86_64-linux-gnu
printf "[binaries]\npkg-config = ['pkg-config', '--define-prefix']\nllvm-config = 'llvm-config-21'\n" > "$M/native.ini"
[ -f "$M/build/build.ninja" ] || meson setup "$M/build" "$M/src" --native-file "$M/native.ini" \
    --prefix "$M/install" -Dbuildtype=release -Dvulkan-drivers=swrast -Dgallium-drivers=llvmpipe \
    -Dplatforms=x11 -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Degl=disabled -Dglx=disabled \
    -Dgbm=disabled -Dllvm=enabled -Dshared-llvm=enabled -Dvideo-codecs= -Dvulkan-layers= -Dtools= \
    -Dbuild-tests=false -Dvalgrind=disabled -Dlibunwind=disabled -Dzstd=disabled -Dxlib-lease=disabled \
    -Dgallium-va=disabled -Dgallium-mediafoundation=disabled -Dgallium-rusticl=false \
    -Dmicrosoft-clc=disabled -Dspirv-tools=disabled -Dlmsensors=disabled -Dexpat=disabled > "$M/setup.log" 2>&1 ||
    { tail -n 20 "$M/setup.log" >&2; exit 1; }
ninja -C "$M/build" -j "${JOBS:-6}" > "$M/ninja.log" 2>&1 || { tail -n 30 "$M/ninja.log" >&2; exit 1; }
ninja -C "$M/build" install > /dev/null
echo "$MESA_COMMIT" > "$M/install/.commit"
echo "$ICD"
