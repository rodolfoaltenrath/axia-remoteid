#!/bin/sh
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$(dirname -- "$script_directory")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/desktopid-rust-target}"
command -v pkcs11-tool >/dev/null
cargo build --workspace --locked
cargo xtask pkcs11-smoke --module "$CARGO_TARGET_DIR/debug/libdesktopid_pkcs11.so" \
    --client "$CARGO_TARGET_DIR/debug/desktopid-client" \
    --report "test-results/pkcs11-$(date -u +%Y%m%d-%H%M%S).json"
