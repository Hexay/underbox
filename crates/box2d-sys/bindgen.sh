#!/bin/sh
# Regenerates src/bindings.rs from third_party/box2d's public headers (needs bindgen-cli + libclang).
# Committed output, so building never needs libclang (Windows CI has none).
set -eu
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
INC="$HERE/../../third_party/box2d/include"
bindgen "$INC/box2d/box2d.h" \
    --rust-edition 2024 --rust-target 1.85 \
    --allowlist-item 'b2.*' --allowlist-item 'B2_.*' \
    --no-doc-comments --merge-extern-blocks \
    --output "$HERE/src/bindings.rs" \
    -- -I"$INC"
echo "wrote $HERE/src/bindings.rs ($(wc -l < "$HERE/src/bindings.rs") lines)"
