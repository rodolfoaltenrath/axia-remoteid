"""Install only the compiled provider/helper in private Linux user directories."""
import argparse
import os
from pathlib import Path
import shutil
import stat

def install(target,home):
    home=home.resolve(strict=True)
    if str(home).startswith('/mnt/'):
        raise RuntimeError('Install on the Linux filesystem')
    files=[(target/'libdesktopid_pkcs11.so',home/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'),
           (target/'desktopid-client',home/'.local/libexec/desktopid-linux/desktopid-client')]
    for source,destination in files:
        if source.read_bytes()[:4]!=b'\x7fELF': raise RuntimeError('Expected native ELF artifact')
        destination.parent.mkdir(parents=True,exist_ok=True)
        if not destination.parent.resolve().is_relative_to(home):
            raise RuntimeError('Install directory resolves outside Linux home')
        metadata=destination.parent.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid!=os.geteuid():
            raise RuntimeError('Install directory is not an owned directory')
        destination.parent.chmod(0o700)
        if destination.exists() or destination.is_symlink():
            old=destination.lstat()
            if not stat.S_ISREG(old.st_mode) or old.st_uid!=os.geteuid():
                raise RuntimeError('Refusing to replace foreign installation file')
        temporary=destination.with_suffix(destination.suffix+'.pending')
        with temporary.open('xb') as output:
            os.chmod(temporary,0o700)
            with source.open('rb') as input_file: shutil.copyfileobj(input_file,output)
            output.flush(); os.fsync(output.fileno())
        temporary.replace(destination)
        print(destination)

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target',type=Path,required=True)
    parser.add_argument('--home',type=Path,default=Path.home())
    args=parser.parse_args()
    install(args.target,args.home)
