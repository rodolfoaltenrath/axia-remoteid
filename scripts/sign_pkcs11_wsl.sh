#!/bin/sh
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$(dirname -- "$script_directory")"
exec cargo xtask sign-pkcs11-test "$@"
