# Sourced by tools/boot-client.sh and tools/diff/real-client.sh for a Facepunch engine2 client in run
# copy $GAME (docs/differential.md, "Real's UI on lavapipe"). Exports VK_ICD_FILENAMES, SDL_VIDEO_DRIVER,
# LD_LIBRARY_PATH and the closed proxy (noweb.sh); sets STANDIN (for LD_PRELOAD).
# VK_ICD_FILENAMES: as given, else tools/diff/lavapipe.sh's build if present, else the system lavapipe.
if [ -z "${VK_ICD_FILENAMES:-}" ]; then
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
    for f in "${UNDERBOX_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/underbox}"/mesa-lvp/install/share/vulkan/icd.d/lvp_icd.*.json; do
        [ -f "$f" ] && VK_ICD_FILENAMES=$f
    done
fi
export VK_ICD_FILENAMES SDL_VIDEO_DRIVER=x11
STANDIN=$(sh "$ROOT/tools/diff/standin.sh")
[ -z "${UNDERBOX_STANDIN_FRIENDS:-}" ] || sh "$ROOT/tools/diff/standin.sh" --shim "$GAME/bin/linuxsteamrt64"
# libsteam_api dlopens a bare "steamclient.so" before trying ~/.steam.
export LD_LIBRARY_PATH=$GAME/bin/linuxsteamrt64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
# Managed's menu calls sbox.game's web API once its UI ticks, and games fetch packages.
. "$ROOT/tools/diff/noweb.sh"
