"""Read-only environment inventory for the future Fedora test; no account access."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess


def collect(module=None):
    result = {'system': platform.system(), 'architecture': platform.machine(),
              'pointer_bits': ctypes.sizeof(ctypes.c_void_p) * 8,
              'unsigned_long_bits': ctypes.sizeof(ctypes.c_ulong) * 8,
              'session_type': os.environ.get('XDG_SESSION_TYPE'),
              'desktop': os.environ.get('XDG_CURRENT_DESKTOP'),
              'python': platform.python_version()}
    result['kernel_release'] = platform.release()
    result['is_wsl'] = (result['system'] == 'Linux' and
        ('microsoft' in result['kernel_release'].lower() or
         'wsl' in result['kernel_release'].lower() or bool(os.environ.get('WSL_INTEROP'))))
    result['runtime'] = 'wsl' if result['is_wsl'] else ('linux' if result['system'] == 'Linux' else 'windows' if result['system'] == 'Windows' else result['system'].lower())
    os_release = Path('/etc/os-release')
    if os_release.exists():
        result['os_release'] = dict(line.split('=', 1) for line in
            os_release.read_text().splitlines() if '=' in line and not line.startswith('#'))
    commands = ['firefox', 'p11-kit', 'modutil', 'certutil', 'pkcs11-tool',
                'openssl', 'readelf', 'gcc', 'make', 'pkg-config', 'flatpak']
    result['commands_available'] = {command: bool(shutil.which(command)) for command in commands}
    if result['system'] == 'Linux' and shutil.which('rpm'):
        packages = ['firefox', 'p11-kit', 'p11-kit-devel', 'nss-tools', 'opensc',
                    'openssl', 'openssl-devel', 'gcc', 'make', 'python3-cryptography']
        result['packages'] = {}
        for package in packages:
            query = subprocess.run(['rpm', '-q', package], capture_output=True, text=True, timeout=10)
            result['packages'][package] = query.stdout.strip() if query.returncode == 0 else None
    if result['system'] == 'Linux':
        native = [Path.home()/'.mozilla/firefox', Path.home()/'.config/mozilla/firefox']
        result['native_firefox_profile_directory_present'] = any(p.is_dir() for p in native)
        result['flatpak_firefox_directory_present'] = (Path.home()/'.var/app/org.mozilla.firefox').is_dir()
    if module:
        path = Path(module)
        with path.open('rb') as file:
            header = file.read(32)
        module_info = {'size_bytes': path.stat().st_size, 'is_elf': header[:4] == b'\x7fELF'}
        if module_info['is_elf'] and len(header) >= 20 and header[4] in (1, 2) and header[5] in (1, 2):
            module_info['bits'] = 32 if header[4] == 1 else 64
            module_info['machine_id'] = struct.unpack_from('<H' if header[5] == 1 else '>H', header, 18)[0]
            module_info['matches_process_bits'] = module_info['bits'] == result['pointer_bits']
        result['module'] = module_info
    result['note'] = ('Este inventário não testa cadastro, 2FA ou assinatura. '
                      'DLL e dylib extraídas não são módulos ELF para o Fedora.')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--module', help='Future native .so to inspect without loading it')
    args = parser.parse_args()
    print(json.dumps(collect(args.module), indent=2, ensure_ascii=False))
