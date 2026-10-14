"""Register the installed PKCS11 module in one closed native Firefox profile.

Uses the system NSS library, including the same module-registration API as
modutil. Never reads passwords or requests a remote signature. NSS references:
https://github.com/nss-dev/nss/blob/master/lib/pk11wrap/secmod.h
https://github.com/mozilla/gecko-dev/blob/master/toolkit/profile/nsProfileLock.cpp
"""
import argparse
import configparser
from contextlib import contextmanager
import ctypes as C
import ctypes.util
import fcntl
import json
import os
from pathlib import Path
import stat
import uuid

# Preserve the registered NSS nickname across branding updates.
MODULE_NAME = 'DesktopID Linux Alpha'
TOKEN_NAME = 'Axia RemotedID'
DATABASE_FILES = ('pkcs11.txt', 'cert9.db', 'key4.db')


def owned_file(path):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid():
        raise RuntimeError('Expected an owned regular file: ' + str(path))
    return metadata


def default_profile(home):
    home = home.resolve(strict=True)
    config_root = Path(os.environ.get('XDG_CONFIG_HOME', str(home/'.config')))
    roots = [config_root/'mozilla/firefox', home/'.mozilla/firefox']
    candidates = []
    for root in dict.fromkeys(roots):
        ini = root/'profiles.ini'
        if not ini.is_file():
            continue
        parser = configparser.ConfigParser(interpolation=None)
        parser.read(ini)
        installed = [parser[section].get('Default') for section in parser.sections()
                     if section.startswith('Install') and parser[section].get('Default')]
        if installed:
            paths = [root/path for path in installed]
        else:
            paths = []
            for section in parser.sections():
                value = parser[section]
                if section.startswith('Profile') and value.get('Default') == '1':
                    path = Path(value['Path'])
                    paths.append(root/path if value.get('IsRelative', '1') == '1' else path)
        candidates.extend(path.resolve(strict=True) for path in paths if path.is_dir())
    candidates = list(dict.fromkeys(candidates))
    if len(candidates) != 1:
        raise RuntimeError('Could not select one native Firefox profile; pass --profile explicitly')
    return candidates[0]


@contextmanager
def profile_lock(profile):
    """Hold the same fcntl lock used by native Firefox, until NSS is closed."""
    descriptor = os.open(profile/'.parentlock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        metadata = os.fstat(descriptor)
        if metadata.st_uid != os.geteuid() or not stat.S_ISREG(metadata.st_mode):
            raise RuntimeError('Firefox profile lock is not an owned file')
        try:
            fcntl.lockf(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('Feche o Firefox para concluir a integração; o perfil está em uso') from None
        legacy = profile/'lock'
        if legacy.is_symlink() and ':+' not in os.readlink(legacy):
            raise RuntimeError('Firefox profile has a legacy lock; close the browser before integrating')
        yield
    finally:
        os.close(descriptor)


class ListLinks(C.Structure):
    _fields_ = [('next', C.c_void_p), ('previous', C.c_void_p)]


class NssDatabase:
    def __init__(self, profile):
        self.profile = profile
        library = ctypes.util.find_library('nss3')
        if not library:
            raise RuntimeError('System NSS library not found; install the native Firefox package')
        self.library = C.CDLL(library)
        definitions = [
            ('NSS_InitReadWrite', [C.c_char_p], C.c_int),
            ('NSS_Shutdown', [], C.c_int),
            ('PORT_GetError', [], C.c_int),
            ('SECMOD_AddNewModule', [C.c_char_p, C.c_char_p, C.c_ulong, C.c_ulong], C.c_int),
            ('SECMOD_FindModule', [C.c_char_p], C.c_void_p),
            ('SECMOD_DestroyModule', [C.c_void_p], None),
            ('PK11_FindSlotByName', [C.c_char_p], C.c_void_p),
            ('PK11_FreeSlot', [C.c_void_p], None),
            ('PK11_ListCertsInSlot', [C.c_void_p], C.c_void_p),
            ('CERT_DestroyCertList', [C.c_void_p], None),
        ]
        for name, arguments, result in definitions:
            function = getattr(self.library, name)
            function.argtypes = arguments
            function.restype = result

    def check(self, status, operation):
        if status != 0:
            raise RuntimeError(f'{operation} failed (NSS error {self.library.PORT_GetError()})')

    def __enter__(self):
        self.check(self.library.NSS_InitReadWrite(os.fsencode('sql:' + str(self.profile))), 'NSS initialization')
        return self

    def __exit__(self, kind, value, traceback):
        status = self.library.NSS_Shutdown()
        if kind is None:
            self.check(status, 'NSS shutdown')

    def loaded(self):
        module = self.library.SECMOD_FindModule(MODULE_NAME.encode())
        if module:
            self.library.SECMOD_DestroyModule(module)
        return bool(module)

    def add(self, module):
        # Zero flags: do not change the default crypto provider or trust settings.
        self.check(self.library.SECMOD_AddNewModule(MODULE_NAME.encode(), os.fsencode(module), 0, 0),
                   'PKCS11 module registration')
        if not self.loaded():
            raise RuntimeError('NSS did not load the registered module')

    def certificate_count(self):
        slot = self.library.PK11_FindSlotByName(TOKEN_NAME.encode())
        if not slot:
            raise RuntimeError('NSS could not find the Axia RemotedID token')
        certificates = None
        try:
            certificates = self.library.PK11_ListCertsInSlot(slot)
            if not certificates:
                raise RuntimeError('NSS could not enumerate certificates')
            node = C.cast(certificates, C.POINTER(ListLinks)).contents.next
            count = 0
            while node != certificates:
                if not node or count >= 4096:
                    raise RuntimeError('Invalid NSS certificate list')
                count += 1
                node = C.cast(node, C.POINTER(ListLinks)).contents.next
            return count
        finally:
            if certificates:
                self.library.CERT_DestroyCertList(certificates)
            self.library.PK11_FreeSlot(slot)


def registered_library(profile):
    file = profile/'pkcs11.txt'
    if not file.exists():
        return None
    owned_file(file)
    for section in file.read_text().split('\n\n'):
        values = dict(line.split('=', 1) for line in section.splitlines() if '=' in line)
        if values.get('name') == MODULE_NAME:
            return values.get('library', '')
    return None


def backup_databases(profile, home):
    root = home/'.local/state/desktopid-linux/browser-backups'
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    if not root.resolve().is_relative_to(home) or root.stat().st_uid != os.geteuid():
        raise RuntimeError('Invalid private browser backup directory')
    root.chmod(0o700)
    directory = root/uuid.uuid4().hex
    directory.mkdir(mode=0o700)
    for name in DATABASE_FILES:
        source = profile/name
        if source.exists():
            owned_file(source)
            with (directory/name).open('xb') as output:
                os.chmod(directory/name, 0o600)
                output.write(source.read_bytes())
                output.flush()
                os.fsync(output.fileno())
    return directory


def register(profile, module, home):
    home = home.resolve(strict=True)
    profile = profile.resolve(strict=True)
    if not profile.is_dir() or not profile.is_relative_to(home) or profile.stat().st_uid != os.geteuid():
        raise RuntimeError('Firefox profile must be an owned directory inside the Linux home')
    module = module.absolute()
    metadata = owned_file(module)
    if metadata.st_mode & 0o022 or not module.resolve().is_relative_to(home):
        raise RuntimeError('Expected a protected, user-installed PKCS11 module')
    if any(character in str(module) for character in '\r\n\0'):
        raise RuntimeError('Invalid module path')
    for name in DATABASE_FILES:
        owned_file(profile/name)
    with profile_lock(profile):
        existing = registered_library(profile)
        if existing is not None and existing != str(module):
            raise RuntimeError('A different module already uses the DesktopID Alpha name; it was preserved')
        backup = backup_databases(profile, home) if existing is None else None
        with NssDatabase(profile) as database:
            if existing is None:
                database.add(module)
            elif not database.loaded():
                raise RuntimeError('The configured module could not be loaded')
            count = database.certificate_count()
        if registered_library(profile) != str(module):
            raise RuntimeError('NSS did not persist the module path')
    return {'browser': 'native Firefox', 'module': MODULE_NAME, 'profile': str(profile),
            'registered_now': existing is None, 'module_loaded_by_nss': True,
            'certificate_count': count, 'backup_directory': str(backup) if backup else None,
            'remote_signature_requested': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', type=Path)
    parser.add_argument('--home', type=Path, default=Path.home())
    parser.add_argument('--module', type=Path)
    args = parser.parse_args()
    module = args.module or args.home/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'
    try:
        profile = args.profile or default_profile(args.home)
        print(json.dumps(register(profile, module, args.home), indent=2))
    except Exception as error:
        parser.exit(1, f'Erro: {error}\n')
