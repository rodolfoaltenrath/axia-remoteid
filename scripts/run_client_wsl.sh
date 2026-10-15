#!/bin/sh
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$(dirname -- "$script_directory")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/desktopid-rust-target}"
exec cargo run --quiet --locked -p axia-desktop --bin desktopid-client -- "$@"
