"""Isolated-container test of the root-owned RPM runtime, with synthetic account data."""
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from cryptography import x509
from cryptography.hazmat.primitives.serialization import Encoding

def main():
    sys.path.insert(0, '/usr/share/axia-remotedid')
    import desktop_app
    from firefox_pkcs11 import NssDatabase, SYSTEM_MODULE, protected_system_module, register, registered_library
    assert os.geteuid() != 0
    assert protected_system_module(SYSTEM_MODULE)
    helper = Path('/usr/libexec/axia-remotedid/desktopid-client')
    launcher = Path('/usr/bin/axia-remotedid')
    subprocess.run([str(launcher), '--version'], check=True)
    home = Path.home()
    state = home/'.config/desktopid-linux'
    subprocess.run([str(helper), 'init', '--server', 'https://example.invalid', '--access-url', ''],
                   check=True, capture_output=True)
    config = json.loads((state/'config.json').read_text())
    config['desktop_code'] = 'synthetic-lab-code'
    (state/'config.json').write_text(json.dumps(config))
    fixture = json.loads((Path(__file__).resolve().parents[1]/
                         'crates/desktopid-client/tests/fixtures/python-vectors.json').read_text())
    certificate = x509.load_pem_x509_certificate(fixture['certificate_pem'].encode())
    wallet = {'schema_version': 1, 'public_key_sha256': config['public_key_sha256'],
              'desktop_code': config['desktop_code'], 'certificados': [{
                  'index': 0, 'keyName': 'SERIAL;ISSUER',
                  'base64': base64.b64encode(certificate.public_bytes(Encoding.DER)).decode()}]}
    (state/'wallet.json').write_text(json.dumps(wallet))
    (state/'wallet.json').chmod(0o600)
    os.environ['DESKTOPID_STATE_DIR'] = str(state)
    profile_root = home/'.config/mozilla/firefox'
    profile = profile_root/'laboratory'
    profile.mkdir(parents=True, mode=0o700)
    (profile_root/'profiles.ini').write_text('[Profile0]\nPath=laboratory\nName=Laboratory\nDefault=1\n')
    with NssDatabase(profile):
        pass
    legacy = home/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'
    legacy.parent.mkdir(parents=True)
    shutil.copyfile(SYSTEM_MODULE, legacy)
    legacy.chmod(0o700)
    register(profile, legacy, home)
    snapshot = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in state.iterdir() if path.is_file()}
    migrated = register(profile, SYSTEM_MODULE, home, migrate=True)
    assert migrated['migrated_legacy_module']
    assert migrated['certificate_count'] == 1
    assert migrated['backup_directory']
    assert registered_library(profile) == str(SYSTEM_MODULE)
    again = register(profile, SYSTEM_MODULE, home, migrate=True)
    assert not again['registered_now'] and not again['migrated_legacy_module']
    assert not again['backup_directory']
    desktop_app.dialog = lambda *args, **kwargs: 0
    assert desktop_app.setup_firefox(ask=False) == 0
    subprocess.run([str(launcher), 'autostart', '--enable'], check=True)
    entry = home/'.config/autostart/axia-remotedid.desktop'
    assert str(helper) in entry.read_text(), 'Startup did not choose the protected RPM helper'
    snapshot[str(entry)] = hashlib.sha256(entry.read_bytes()).hexdigest()
    preferences = home/'.config/axia-remotedid/preferences.json'
    snapshot[str(preferences)] = hashlib.sha256(preferences.read_bytes()).hexdigest()
    for path, digest in snapshot.items():
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest
    (home/'rpm-lab-snapshot.json').write_text(json.dumps(snapshot))
    print('System RPM runtime: command, protected library, Firefox migration, backup, idempotence, graphical setup and system autostart passed; no cloud requests.')


def check_preserved(home):
    snapshot = json.loads((home/'rpm-lab-snapshot.json').read_text())
    for path, digest in snapshot.items():
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest
    assert not Path('/usr/libexec/axia-remotedid/desktopid-client').exists()
    assert not Path('/usr/lib64/axia-remotedid/libdesktopid_pkcs11.so').exists()
    print('RPM removal verified: runtime removed; synthetic identity, wallet, preferences and startup entry preserved.')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--check-preserved':
        check_preserved(Path(sys.argv[2]))
    else:
        main()
