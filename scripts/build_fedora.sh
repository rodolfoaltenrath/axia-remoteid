#!/bin/sh
# Build local test artifacts; target/ and dist/ are ignored by Git.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_directory=$(dirname -- "$script_directory")
cd "$project_directory"

for dependency in cargo rustc gcc pkg-config; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        printf 'Dependência ausente: %s\n' "$dependency" >&2
        printf 'Instale com: sudo dnf install rust cargo gcc pkgconf-pkg-config openssl-devel\n' >&2
        exit 1
    fi
done
if ! pkg-config --exists openssl; then
    printf 'Instale os arquivos de desenvolvimento: sudo dnf install openssl-devel\n' >&2
    exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$project_directory/target}"
cargo build --release --workspace --locked
destination="$project_directory/dist/fedora"
mkdir -p "$destination"
install -m 700 "$CARGO_TARGET_DIR/release/desktopid-client" "$destination/desktopid-client"
install -m 700 "$CARGO_TARGET_DIR/release/desktopid-client" "$destination/axia-remotedid"
install -m 700 "$CARGO_TARGET_DIR/release/libdesktopid_pkcs11.so" "$destination/libdesktopid_pkcs11.so"
"$destination/desktopid-client" --version
printf '\nArquivos de teste em: %s\n' "$destination"
printf 'Interface gráfica: ./dist/fedora/axia-remotedid\n'
printf 'Ajuda: ./dist/fedora/axia-remotedid --help\n'
