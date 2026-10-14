"""Install the alpha runtime, GNOME launcher and native Firefox integration."""
import argparse
import os
from pathlib import Path
import stat
import uuid

if __package__:
    from .install_pkcs11 import install
    from .firefox_pkcs11 import default_profile, register
else:
    from install_pkcs11 import install
    from firefox_pkcs11 import default_profile, register


def desktop_argument(value):
    if any(character in value for character in '\r\n\0'):
        raise RuntimeError('Invalid desktop launcher path')
    value = value.replace('\\', '\\\\\\\\').replace('%', '%%')
    for character in ('"', '`', '$'):
        value = value.replace(character, '\\\\' + character)
    return '"' + value + '"'


def install_launcher(home):
    home = home.resolve(strict=True)
    destination = home/'.local/share/applications/axia-remotedid-alpha.desktop'
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not destination.parent.resolve().is_relative_to(home):
        raise RuntimeError('Launcher directory resolves outside home')
    if destination.exists() or destination.is_symlink():
        metadata = destination.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid():
            raise RuntimeError('Refusing to replace a foreign launcher')
    binary = home/'.local/libexec/desktopid-linux/desktopid-client'
    # A new desktop ID lets GNOME recreate icons still holding the old name.
    # Migrate only our own launcher; keep unrelated files intact.
    legacy = destination.with_name('desktopid-linux-alpha.desktop')
    if legacy.exists() and not legacy.is_symlink():
        metadata = legacy.lstat()
        if stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.geteuid():
            lines = legacy.read_text().splitlines()
            if (f'Exec={desktop_argument(str(binary))} gui' in lines
                    and any(line in lines for line in ('Name=DesktopID Linux Alpha',
                                                      'Name=Axia RemotedID Alpha'))):
                if not destination.exists():
                    legacy.replace(destination)
                else:
                    legacy.unlink()
    content = ('[Desktop Entry]\nType=Application\nVersion=1.0\n'
               'Name=Axia RemotedID Alpha\n'
               'Comment=Conectar certificados em nuvem Certisign\n'
               f'Exec={desktop_argument(str(binary))} gui\n'
               f'TryExec={binary}\n'
               'Icon=dialog-password\nTerminal=false\nCategories=Utility;Security;\n')
    temporary = destination.with_name(destination.name + '.' + uuid.uuid4().hex + '.pending')
    with temporary.open('x') as output:
        os.chmod(temporary, 0o644)
        output.write(content)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(destination)
    # A branded command points to the existing helper path, preserving its ABI
    # and the already registered browser/account configuration.
    directory = home/'.local/bin'
    directory.mkdir(parents=True, exist_ok=True)
    if not directory.resolve().is_relative_to(home) or directory.stat().st_uid != os.geteuid():
        raise RuntimeError('Invalid user command directory')
    command = directory/'axia-remotedid'
    if command.exists() or command.is_symlink():
        if not command.is_symlink() or command.lstat().st_uid != os.geteuid() or command.resolve() != binary.resolve():
            raise RuntimeError('Refusing to replace a foreign axia-remotedid command')
    else:
        command.symlink_to(binary)
    print('Atalho instalado: ' + str(destination))
    return destination


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', type=Path, required=True)
    parser.add_argument('--home', type=Path, default=Path.home())
    parser.add_argument('--profile', type=Path)
    parser.add_argument('--skip-firefox', action='store_true', help='Install runtime/menu only')
    args = parser.parse_args()
    try:
        install(args.target, args.home)
        install_launcher(args.home)
        if not args.skip_firefox:
            profile = args.profile or default_profile(args.home)
            module = args.home/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'
            result = register(profile, module, args.home)
            print(f'Firefox integrado; {result["certificate_count"]} certificado(s) disponíveis pelo NSS.')
            if result['backup_directory']:
                print('Backup privado: ' + result['backup_directory'])
        print('Alpha instalada. Abra Axia RemotedID Alpha no menu do GNOME.')
    except Exception as error:
        parser.exit(1, f'Erro: {error}\n')
