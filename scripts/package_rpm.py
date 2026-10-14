"""Build a Fedora 44 x86_64 RPM from tested native runtime artifacts."""
import argparse
import hashlib
import io
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile


def build(target, destination, builder, container=False):
    root = Path(__file__).resolve().parents[1]
    if (platform.freedesktop_os_release().get('ID'),
            platform.freedesktop_os_release().get('VERSION_ID'), platform.machine()) != ('fedora', '44', 'x86_64'):
        raise RuntimeError('Build this distribution RPM on Fedora 44 x86_64')
    destination = destination.resolve()
    top = destination/'rpm-build'
    for directory in ['BUILD', 'BUILDROOT', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS']:
        (top/directory).mkdir(parents=True, exist_ok=True)
    entries = {
        'desktopid-client': target/'desktopid-client',
        'libdesktopid_pkcs11.so': target/'libdesktopid_pkcs11.so',
        'axia-remotedid': root/'packaging/axia-remotedid',
        'axia-remotedid-alpha.desktop': root/'packaging/axia-remotedid-alpha.desktop',
        'desktop_app.py': root/'scripts/desktop_app.py',
        'firefox_pkcs11.py': root/'scripts/firefox_pkcs11.py',
        'README.md': root/'README.md',
        'docs/TESTING.md': root/'docs/TESTING.md',
        'docs/DEVELOPMENT.md': root/'docs/DEVELOPMENT.md',
        'docs/PROTOCOL.md': root/'docs/PROTOCOL.md',
        'CONTINUIDADE_IA_FEDORA.md': root/'CONTINUIDADE_IA_FEDORA.md',
        'LICENSE': root/'LICENSE', 'NOTICE': root/'NOTICE',
    }
    for name in ['desktopid-client', 'libdesktopid_pkcs11.so']:
        if entries[name].read_bytes()[:4] != b'\x7fELF':
            raise RuntimeError('Expected native release ELF artifacts')
    source = top/'SOURCES/axia-remotedid-runtime.tar.gz'
    with tarfile.open(source, 'w:gz') as archive:
        for name, path in sorted(entries.items()):
            data = path.read_bytes()
            info = tarfile.TarInfo('axia-remotedid-runtime/'+name)
            info.size = len(data)
            info.mode = 0o755 if name in ['desktopid-client', 'libdesktopid_pkcs11.so', 'axia-remotedid'] else 0o644
            archive.addfile(info, io.BytesIO(data))
    spec = top/'SPECS/axia-remotedid.spec'
    shutil.copyfile(root/'packaging/axia-remotedid.spec', spec)
    if container:
        subprocess.run(['podman', 'run', '--rm', '--security-opt', 'label=disable',
                        '-v', str(top/'SOURCES')+':/build/SOURCES:ro',
                        '-v', str(top/'SPECS')+':/build/SPECS:ro',
                        '-v', str(top/'RPMS')+':/build/RPMS:rw',
                        'registry.fedoraproject.org/fedora:44', '/bin/sh', '-ec',
                        'dnf -y install rpm-build desktop-file-utils openssl-libs; '
                        'rpmbuild -bb --define "_topdir /build" --define "dist .fc44" /build/SPECS/axia-remotedid.spec'],
                       check=True)
    else:
        subprocess.run([builder, '-bb', '--define', '_topdir '+str(top),
                        '--define', 'dist .fc44', str(spec)], check=True)
    packages = list((top/'RPMS/x86_64').glob('axia-remotedid-*.rpm'))
    if len(packages) != 1:
        raise RuntimeError('Expected exactly one runtime RPM')
    output = destination/packages[0].name
    shutil.copyfile(packages[0], output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(output.suffix+'.sha256').write_text(f'{digest}  {output.name}\n')
    print(f'{output}: {output.stat().st_size:,} bytes; SHA256 {digest}; no account state')
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', type=Path, default=Path('dist/fedora'))
    parser.add_argument('--output-dir', type=Path, default=Path('dist'))
    parser.add_argument('--rpmbuild', default=os.environ.get('RPMBUILD', 'rpmbuild'))
    parser.add_argument('--container', action='store_true', help='Build with Podman in a clean Fedora 44 image')
    args = parser.parse_args()
    build(args.target, args.output_dir, args.rpmbuild, args.container)
