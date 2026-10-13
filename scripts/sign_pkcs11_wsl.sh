#!/bin/sh
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$(dirname -- "$script_directory")"
exec python3 -m scripts.sign_pkcs11_test "$@"
