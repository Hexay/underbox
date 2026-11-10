#!/bin/sh
# Slot coverage per native class: subsystem / generated casts / generated fields / stub.
# usage: tools/coverage.sh [--all] [--stubs] [--json] [--hits target/boot/server.log]
# Without --all only classes with stubs are listed (most stubs first); the total line follows.
set -eu
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
# see docs/dev-loop.md (incremental)
exec env -u CARGO_INCREMENTAL cargo run -q --manifest-path "$ROOT/Cargo.toml" -p engine2 --bin coverage -j4 -- "$@"
