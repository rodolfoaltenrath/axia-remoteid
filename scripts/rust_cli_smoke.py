"""Exercise the actual Linux executable with public fixtures and temporary state.

No real host, account, credentials, registration or remote signature is used.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
from cryptography import x509
from cryptography.hazmat.primitives.serialization import Encoding


def run(binary, args, successful=True):
    result = subprocess.run([str(binary), *map(str, args)], capture_output=True,
                            text=True, timeout=30)
    if (result.returncode == 0) != successful:
        raise RuntimeError(f'Unexpected CLI result for {args[0]}: {result.stdout}\n{result.stderr}')
    return result


def smoke(binary):
    root = Path(__file__).resolve().parents[1]
    fixture = json.loads((root/'crates/desktopid-client/tests/fixtures/python-vectors.json').read_text())
    checks = []
    with tempfile.TemporaryDirectory(prefix='desktopid-rust-lab-') as directory:
        directory = Path(directory)
        state = directory/'identity'
        args = ['--state-dir', state, 'init', '--server', 'https://example.invalid', '--access-url', 'laboratory']
        run(binary, args)
        before = (state/'identity.pem').read_bytes()
        assert (state.stat().st_mode & 0o777) == 0o700
        assert ((state/'identity.pem').stat().st_mode & 0o777) == 0o600
        run(binary, args, successful=False)
        assert (state/'identity.pem').read_bytes() == before
        status = run(binary, ['--state-dir', state, 'status'])
        config = json.loads(status.stdout)
        assert config['desktop_code'] is None
        assert 'PRIVATE KEY' not in status.stdout
        checks += ['local-init-private-permissions', 'init-does-not-replace-key', 'status-does-not-print-private-key']
        cert = directory/'lab.pem'
        cert.write_text(fixture['certificate_pem'])
        document = directory/'document.bin'
        document.write_bytes(base64.b64decode(fixture['document_base64']))
        signature = directory/'signature.bin'
        for algorithm in ('SHA256', 'SHA384', 'SHA512'):
            signature.write_bytes(base64.b64decode(fixture['signatures'][algorithm]))
            args = ['verify', '--file', document, '--certificate', cert, '--signature', signature,
                    '--algorithm', algorithm.lower()]
            run(binary, args)
            document.write_bytes(b'changed laboratory document')
            run(binary, args, successful=False)
            document.write_bytes(base64.b64decode(fixture['document_base64']))
            checks.append(f'{algorithm.lower()}-python-signature-and-tamper-rejection')
        run(binary, ['verify', '--file', document, '--certificate', cert, '--signature', signature,
                     '--algorithm', 'sha1'], successful=False)
        checks.append('unsupported-sha1-rejected-before-operation')
        missing = directory/'login-must-not-create-state'
        headless_environment = {name: value for name, value in os.environ.items()
                                if name not in ('DISPLAY', 'WAYLAND_DISPLAY')}
        for command in ([], ['gui']):
            result = subprocess.run([str(binary), '--state-dir', str(missing), *command],
                                    env=headless_environment, capture_output=True,
                                    text=True, timeout=30)
            assert result.returncode != 0
            assert 'Sessão gráfica indisponível' in result.stderr
            assert not missing.exists()
        checks.append('gui-default-and-explicit-command-reject-headless-session-without-state')
        preference_root = directory/'preferences'
        preference_environment = dict(headless_environment, HOME=str(directory),
                                      XDG_CONFIG_HOME=str(preference_root))
        def preference_command(*command):
            result = subprocess.run([str(binary), '--state-dir', str(missing), *command],
                                    env=preference_environment, capture_output=True,
                                    text=True, timeout=30)
            assert result.returncode == 0, result.stderr
            return result
        preference_command('autostart', '--enable')
        entry = preference_root/'autostart/axia-remotedid.desktop'
        assert 'Name=Axia RemotedID' in entry.read_text()
        assert 'Hidden=false' in entry.read_text()
        preference_command('autostart', '--disable')
        assert 'Hidden=true' in entry.read_text()
        assert 'desativada' in preference_command('autostart').stdout
        preference_command('startup')
        assert not missing.exists()
        assert (state/'identity.pem').read_bytes() == before
        checks.append('autostart-enable-disable-and-disabled-startup-without-account-or-network')
        broker = subprocess.run([str(binary), '--state-dir', str(missing), 'pkcs11-broker'],
                                input='', env=headless_environment, capture_output=True,
                                text=True, timeout=10)
        assert broker.returncode == 0 and not broker.stdout
        assert not missing.exists()
        checks.append('signing-broker-closes-on-parent-pipe-eof-without-state-or-network')
        result = run(binary, ['--state-dir', missing, 'login-check', '--server', 'http://example.invalid'], successful=False)
        assert 'HTTPS' in result.stderr
        assert not missing.exists()
        result = run(binary, ['--state-dir', missing, 'login-check', '--email', 'lab@example.invalid'], successful=False)
        assert 'terminal interativo' in result.stderr
        assert not missing.exists()
        checks += ['login-check-rejects-http-before-credentials', 'login-check-requires-local-terminal-and-creates-no-state']
        result = run(binary, ['--state-dir', missing, 'connect'], successful=False)
        assert 'terminal interativo' in result.stderr
        assert not missing.exists()
        checks.append('connect-requires-interactive-terminal-before-creating-identity')
        config['desktop_code'] = 'lab-desktop'
        (state/'config.json').write_text(json.dumps(config))
        wallet = {'schema_version': 1, 'public_key_sha256': config['public_key_sha256'],
                  'desktop_code': 'lab-desktop', 'certificados': [{
                      'index': 0, 'keyName': 'LAB-SERIAL;LAB-ISSUER',
                      'base64': base64.b64encode(x509.load_pem_x509_certificate(
                          fixture['certificate_pem'].encode()).public_bytes(Encoding.DER)).decode()
                  }]}
        wallet_path = state/'wallet.json'
        wallet_path.write_text(json.dumps(wallet))
        wallet_path.chmod(0o600)
        listing = run(binary, ['--state-dir', state, 'sign-test', '--list'])
        assert 'RSA' in listing.stdout
        assert 'apto ao teste' in listing.stdout
        assert 'LAB-SERIAL' not in listing.stdout
        assert 'LAB-ISSUER' not in listing.stdout
        assert 'PRIVATE KEY' not in listing.stdout
        result = run(binary, ['--state-dir', state, 'sign-test'], successful=False)
        assert 'terminal interativo' in result.stderr
        assert not (state/'signing-tests').exists()
        assert (state/'identity.pem').read_bytes() == before
        checks += ['sign-test-lists-cached-certificates-without-account-or-personal-reference',
                   'sign-test-requires-interactive-terminal-before-signature-artifacts']
        run_id = '0123456789abcdef0123456789abcdef'
        artifacts = state/'signing-tests'/run_id
        artifacts.mkdir(parents=True, mode=0o700)
        artifacts.parent.chmod(0o700)
        files = {
            'attempt.json': json.dumps({'operation': 'laboratory-signature', 'algorithm': 'SHA256',
                                        'certificate_number': 1}).encode(),
            'document.txt': base64.b64decode(fixture['document_base64']),
            'certificate.pem': fixture['certificate_pem'].encode(),
            'signature-candidate.bin': base64.b64decode(fixture['signatures']['SHA256']),
        }
        for name, data in files.items():
            (artifacts/name).write_bytes(data)
            (artifacts/name).chmod(0o600)
        args = ['--state-dir', state, 'recover-sign-test', '--run-id', run_id]
        (artifacts/'document.txt').write_bytes(b'tampered document')
        run(binary, args, successful=False)
        assert not (artifacts/'result.json').exists()
        (artifacts/'document.txt').write_bytes(files['document.txt'])
        recovered = run(binary, args)
        assert 'Nenhuma operação de rede' in recovered.stdout
        assert json.loads((artifacts/'result.json').read_text())['recovered_offline']
        assert (artifacts/'signature.bin').read_bytes() == files['signature-candidate.bin']
        assert (artifacts/'signature.bin').stat().st_mode & 0o777 == 0o600
        run(binary, args, successful=False)
        run(binary, ['--state-dir', state, 'recover-sign-test', '--run-id', '../identity'], successful=False)
        checks += ['recover-sign-test-verifies-offline-and-rejects-tampered-document',
                   'recover-sign-test-preserves-existing-results-and-rejects-path-traversal']
    binary_bytes = binary.read_bytes()
    return {'schema_version': 1, 'successful': True, 'scope': 'rust-laboratory-client',
            'system': platform.system(), 'kernel': platform.release(),
            'binary_is_elf': binary_bytes[:4] == b'\x7fELF',
            'binary_sha256': hashlib.sha256(binary_bytes).hexdigest(),
            'checks': checks, 'cloud_login_tested': False, 'cloud_registration_tested': False,
            'cloud_signature_tested': False, 'pkcs11_provider_tested': False, 'gui_tested': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    report = smoke(args.binary)
    report['rust_version'] = subprocess.check_output(['rustc', '--version'], text=True).strip()
    report['cargo_version'] = subprocess.check_output(['cargo', '--version'], text=True).strip()
    if os.environ.get('DESKTOPID_RUST_VALIDATION') == 'passed':
        report.update(cargo_fmt_check_passed=True, cargo_clippy_passed=True, cargo_test_passed=True,
                      cargo_build_passed=True,
                      rust_tests_passed=int(os.environ.get("DESKTOPID_RUST_TESTS", 0)))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2)+'\n')
    print(f'Linux executable: {len(report["checks"])} checks passed; report: {args.report}')
