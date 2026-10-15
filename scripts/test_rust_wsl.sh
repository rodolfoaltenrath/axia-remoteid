#!/bin/sh
# Also usable directly on native Fedora. Never accesses a real cloud account.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_directory=$(dirname -- "$script_directory")
cd "$project_directory"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/desktopid-rust-target}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
# Keep the output so the report records the real number of tests instead of a
# constant that silently goes stale when tests are added. Only POSIX sh tools
# are used here: this image has no awk.
test_log=$(mktemp)
trap 'rm -f "$test_log"' EXIT
# POSIX sh reports only the last command's status in a pipeline; `cargo test |
# tee` could therefore hide a failed test suite. Preserve Cargo's status first.
if cargo test --workspace --locked > "$test_log" 2>&1; then
    cat "$test_log"
else
    cat "$test_log"
    exit 1
fi
rust_tests=0
for count in $(sed -n 's/^test result: ok\. \([0-9]\{1,\}\) passed.*/\1/p' "$test_log"); do
    rust_tests=$((rust_tests + count))
done
cargo build --workspace --locked
report="test-results/rust-$(date -u +%Y%m%d-%H%M%S).json"
DESKTOPID_RUST_VALIDATION=passed DESKTOPID_RUST_TESTS="$rust_tests" \
    cargo xtask cli-smoke \
    --binary "$CARGO_TARGET_DIR/debug/desktopid-client" --report "$report"
