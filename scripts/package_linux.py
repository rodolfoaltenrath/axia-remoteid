"""Small experimental Linux runtime archive; never include account state."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import tomllib

def build(target,destination):
    root=Path(__file__).resolve().parents[1]
    entries={'libdesktopid_pkcs11.so':(target/'libdesktopid_pkcs11.so').read_bytes(),
             'desktopid-client':(target/'desktopid-client').read_bytes(),
             'axia-remotedid':(target/'desktopid-client').read_bytes(),
             'install_linux.py':(root/'scripts/install_pkcs11.py').read_bytes(),
             'install_pkcs11.py':(root/'scripts/install_pkcs11.py').read_bytes(),
             'install_alpha.py':(root/'scripts/install_alpha.py').read_bytes(),
             'firefox_pkcs11.py':(root/'scripts/firefox_pkcs11.py').read_bytes(),
             'install.sh':(root/'scripts/install_fedora.sh').read_bytes(),
             'LEIA-ME.md':(root/'docs/TESTING.md').read_bytes().replace(b'(DEVELOPMENT.md)', b'(docs/DEVELOPMENT.md)'),
             'README.md':(root/'README.md').read_bytes(),
             'docs/TESTING.md':(root/'docs/TESTING.md').read_bytes(),
             'docs/DEVELOPMENT.md':(root/'docs/DEVELOPMENT.md').read_bytes(),
             'docs/PROTOCOL.md':(root/'docs/PROTOCOL.md').read_bytes(),
             'CONTINUIDADE_IA_FEDORA.md':(root/'CONTINUIDADE_IA_FEDORA.md').read_bytes(),
             'LICENSE':(root/'LICENSE').read_bytes(),
             'NOTICE':(root/'NOTICE').read_bytes()}
    for name in ['libdesktopid_pkcs11.so','desktopid-client']:
        if entries[name][:4]!=b'\x7fELF': raise RuntimeError('Expected native ELF artifacts')
    manifest={'schema_version':1,'product':'Axia RemotedID','scope':'experimental PKCS11 runtime; integrations pending',
        'version':tomllib.loads((root/'Cargo.toml').read_text())['workspace']['package']['version'],
        'built_on':platform.freedesktop_os_release().get('PRETTY_NAME','Linux'),
        'channel':'alpha','machine':platform.machine(),
        'validated_target':{'os':'fedora','version':'44','machine':'x86_64'},
        'runtime_dependencies':['python3','zenity','firefox','nss','openssl-libs','libgcc','zlib','ca-certificates'],
        'includes_account_state':False,
        'files':{name:hashlib.sha256(data).hexdigest() for name,data in entries.items()}}
    entries['manifest.json']=json.dumps(manifest,indent=2).encode()
    destination.parent.mkdir(parents=True,exist_ok=True)
    with destination.open('wb') as raw, gzip.GzipFile(fileobj=raw,filename='',mode='wb',mtime=0) as compressed:
        with tarfile.open(fileobj=compressed,mode='w') as archive:
            for name,data in sorted(entries.items()):
                info=tarfile.TarInfo('axia-remotedid/'+name)
                info.size=len(data); info.mode=0o700 if name.endswith(('.so','.py','.sh')) or name in ('desktopid-client','axia-remotedid') else 0o600
                info.mtime=0; info.uid=info.gid=0
                archive.addfile(info,io.BytesIO(data))
    digest=hashlib.sha256(destination.read_bytes()).hexdigest()
    destination.with_suffix(destination.suffix+'.sha256').write_text(f'{digest}  {destination.name}\n')
    print(f'{destination.name}: {destination.stat().st_size:,} bytes; SHA256 {digest}; no personal state')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target',type=Path,required=True)
    parser.add_argument('--output',type=Path,default=Path('dist/axia-remotedid-0.1.0-alpha.1-fedora44-x86_64.tar.gz'))
    args=parser.parse_args()
    build(args.target,args.output)
