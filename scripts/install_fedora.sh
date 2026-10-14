#!/bin/sh
# End-user entry point for the precompiled Fedora alpha archive.
set -eu
package_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
action="${1:-install}"
if [ "$#" -gt 1 ]; then
    printf 'Use: sh install.sh [--check | --firefox | --help]\n' >&2
    exit 1
fi
case "$action" in
    --help)
        printf 'Axia RemotedID Alpha\n\n'
        printf 'sh install.sh           Instalar o aplicativo no usuário atual\n'
        printf 'sh install.sh --check   Conferir os requisitos sem instalar\n'
        printf 'sh install.sh --firefox Integrar o Firefox após conectar a conta\n'
        exit 0
        ;;
    install|--check|--firefox) ;;
    *) printf 'Opção inválida. Use sh install.sh --help\n' >&2; exit 1 ;;
esac

if [ "$(id -u)" -eq 0 ]; then
    printf 'Execute como seu usuário normal, sem sudo.\n' >&2
    exit 1
fi
if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
    printf 'Este pacote foi preparado para Fedora 44 x86_64.\n' >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    printf 'Instale as dependências: sudo dnf install python3 zenity firefox nss openssl-libs libgcc zlib ca-certificates\n' >&2
    exit 1
fi
python3 - "$package_directory" <<'PY'
import ctypes.util
from pathlib import Path
import platform
import subprocess
import sys

package = Path(sys.argv[1])
system = platform.freedesktop_os_release()
if (system.get('ID'), system.get('VERSION_ID')) != ('fedora', '44'):
    sys.exit('Este pacote foi validado no Fedora 44. Para outro sistema, consulte o guia técnico.')
missing = []
if not Path('/usr/bin/zenity').is_file():
    missing.append('zenity')
if not Path('/usr/bin/firefox').is_file():
    missing.append('firefox nativo')
if not ctypes.util.find_library('nss3'):
    missing.append('nss')
if missing:
    sys.exit('Dependências ausentes: ' + ', '.join(missing) +
             '\nExecute: sudo dnf install python3 zenity firefox nss openssl-libs libgcc zlib ca-certificates')
try:
    subprocess.run([str(package/'axia-remotedid'), '--version'], check=True)
except (OSError, subprocess.CalledProcessError):
    sys.exit('Não foi possível executar a build. Confira as bibliotecas do sistema e extraia o pacote novamente.')
PY
if [ "$action" = --check ]; then
    printf 'Requisitos conferidos. Nenhuma instalação ou conexão foi realizada.\n'
elif [ "$action" = --firefox ]; then
    python3 "$package_directory/firefox_pkcs11.py"
    printf 'Integração concluída. Reabra o Firefox para usar os certificados.\n'
else
    python3 "$package_directory/install_alpha.py" --target "$package_directory" --skip-firefox
    printf '\nAbra Axia RemotedID Alpha no menu e conecte sua conta.\n'
    printf 'Depois feche o Firefox e execute: sh install.sh --firefox\n'
fi
