"""Validate native NSS registration with a synthetic wallet and temporary profile."""
import argparse
import base64
import ctypes as C
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from cryptography import x509
from cryptography.hazmat.primitives import serialization

if __package__:
    from .firefox_pkcs11 import DATABASE_FILES, ListLinks, NssDatabase, register
else:
    from firefox_pkcs11 import DATABASE_FILES, ListLinks, NssDatabase, register


class CertificateNode(C.Structure):
    _fields_ = [('links', ListLinks), ('certificate', C.c_void_p)]


def check_key_without_password_prompt(profile):
    with NssDatabase(profile) as database:
        library = database.library
        for name, arguments, result in [
            ('PK11_NeedLogin', [C.c_void_p], C.c_int),
            ('PK11_Authenticate', [C.c_void_p, C.c_int, C.c_void_p], C.c_int),
            ('PK11_SetPasswordFunc', [C.c_void_p], None),
            ('PK11_FindKeyByAnyCert', [C.c_void_p, C.c_void_p], C.c_void_p),
            ('SECKEY_DestroyPrivateKey', [C.c_void_p], None),
        ]:
            function = getattr(library, name)
            function.argtypes = arguments
            function.restype = result
        prompts = []
        callback_type = C.CFUNCTYPE(C.c_void_p, C.c_void_p, C.c_int, C.c_void_p)
        def password_callback(slot, retry, context):
            prompts.append(True)
            return None
        callback = callback_type(password_callback)
        library.PK11_SetPasswordFunc(C.cast(callback, C.c_void_p))
        slot = library.PK11_FindSlotByName(b'Axia RemotedID')
        certificates = None
        try:
            assert slot
            assert library.PK11_NeedLogin(slot) == 0
            assert library.PK11_Authenticate(slot, 1, None) == 0
            certificates = library.PK11_ListCertsInSlot(slot)
            assert certificates
            node = C.cast(certificates, C.POINTER(ListLinks)).contents.next
            assert node != certificates
            certificate = C.cast(node, C.POINTER(CertificateNode)).contents.certificate
            key = library.PK11_FindKeyByAnyCert(certificate, None)
            assert key, 'NSS did not find the remote key reference without token login'
            library.SECKEY_DestroyPrivateKey(key)
            assert prompts == [], 'NSS requested an unnecessary token login'
        finally:
            library.PK11_SetPasswordFunc(None)
            if certificates:
                library.CERT_DestroyCertList(certificates)
            if slot:
                library.PK11_FreeSlot(slot)


def smoke(module, client):
    fixture = json.loads((Path(__file__).resolve().parents[1]/
                         'crates/desktopid-client/tests/fixtures/python-vectors.json').read_text())
    with tempfile.TemporaryDirectory(prefix='desktopid-firefox-nss-lab-') as directory:
        home = Path(directory)
        state = home/'identity'
        profile = home/'firefox'
        profile.mkdir(mode=0o700)
        subprocess.run([str(client.resolve()), '--state-dir', str(state), 'init',
                        '--server', 'https://example.invalid', '--access-url', ''],
                       check=True, capture_output=True, timeout=30)
        config = json.loads((state/'config.json').read_text())
        config['desktop_code'] = 'synthetic-lab-code'
        (state/'config.json').write_text(json.dumps(config))
        certificate = x509.load_pem_x509_certificate(fixture['certificate_pem'].encode())
        wallet = {'schema_version': 1, 'public_key_sha256': config['public_key_sha256'],
                  'desktop_code': config['desktop_code'], 'certificados': [
                      {'index': 0, 'keyName': 'SERIAL;ISSUER', 'base64': base64.b64encode(
                          certificate.public_bytes(serialization.Encoding.DER)).decode()}]}
        (state/'wallet.json').write_text(json.dumps(wallet))
        (state/'wallet.json').chmod(0o600)
        installed = home/'libdesktopid_pkcs11.so'
        shutil.copyfile(module, installed)
        installed.chmod(0o700)
        original = os.environ.get('DESKTOPID_STATE_DIR')
        os.environ['DESKTOPID_STATE_DIR'] = str(state)
        try:
            with NssDatabase(profile):
                pass
            before = {name: (profile/name).read_bytes() for name in DATABASE_FILES}
            first = register(profile, installed, home)
            assert first['registered_now'] and first['module_loaded_by_nss']
            assert first['certificate_count'] == 1
            backup = Path(first['backup_directory'])
            assert backup.stat().st_mode & 0o777 == 0o700
            for name, contents in before.items():
                assert (backup/name).read_bytes() == contents
                assert (backup/name).stat().st_mode & 0o777 == 0o600
            saved = (profile/'pkcs11.txt').read_bytes()
            second = register(profile, installed, home)
            assert not second['registered_now'] and second['backup_directory'] is None
            assert second['certificate_count'] == 1
            assert (profile/'pkcs11.txt').read_bytes() == saved
            check_key_without_password_prompt(profile)
            conflicting = home/'different-module.so'
            shutil.copyfile(installed, conflicting)
            conflicting.chmod(0o700)
            try:
                register(profile, conflicting, home)
                raise AssertionError('Expected conflicting module registration to fail')
            except RuntimeError as error:
                assert 'different module' in str(error)
            assert (profile/'pkcs11.txt').read_bytes() == saved
        finally:
            if original is None:
                os.environ.pop('DESKTOPID_STATE_DIR', None)
            else:
                os.environ['DESKTOPID_STATE_DIR'] = original
    print('Firefox/NSS: load, certificate/key discovery without password callback, persistence, idempotence, private backup and conflict checks passed; no cloud calls.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--module', type=Path, required=True)
    parser.add_argument('--client', type=Path, required=True)
    args = parser.parse_args()
    smoke(args.module, args.client)
