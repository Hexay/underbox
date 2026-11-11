# Sourced by tools/*.sh: sets and exports the scratch roots kept outside the checkout, creating
# UNDERBOX_CACHE. See docs/dev-loop.md (scratch directories); crates/shaders/src/dxc.rs mirrors the DXC default.
: "${UNDERBOX_CACHE:=${XDG_CACHE_HOME:-$HOME/.cache}/underbox}"
: "${UNDERBOX_GAME_DIRS:=$UNDERBOX_CACHE/underbox-game}"
: "${UNDERBOX_GAME_COPIES:=$UNDERBOX_CACHE/underbox-real-games}"
: "${UNDERBOX_PORT_HOLDS:=$UNDERBOX_CACHE/underbox-ports}"
: "${UNDERBOX_BOOT_LOCK:=$UNDERBOX_CACHE/underbox-boot.lock}"
: "${UNDERBOX_DXC_DIR:=$UNDERBOX_CACHE/dxc}"
export UNDERBOX_CACHE UNDERBOX_GAME_DIRS UNDERBOX_GAME_COPIES UNDERBOX_PORT_HOLDS UNDERBOX_BOOT_LOCK UNDERBOX_DXC_DIR
mkdir -p "$UNDERBOX_CACHE"
