"""Check source publication candidates without displaying private values.

Uses the Git index and untracked, unignored files when this is a repository.
Without Git, checks the source tree excluding generated output. This is a
limited local guard, not a guarantee that arbitrary secrets can be detected.
"""
import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess

GENERATED = {'.git', 'target', 'dist', 'test-results', '__pycache__', '.venv', '.local-research'}
LAB_FIXTURE = 'crates/desktopid-client/tests/fixtures/python-vectors.json'
# Deliberately public, self-signed laboratory key/certificate. A regenerated
# fixture must be reviewed and this exact-file exception updated explicitly.
LAB_SHA256 = '4ad80be20112e24c79abefe0bb80fcd0354bf00ede3e64978d544a4ad71a2770'
PRIVATE_PEM = re.compile(rb'-----BEGIN (?:RSA |EC |ENCRYPTED |OPENSSH )?PRIVATE KEY-----')
JWT = re.compile(rb'\beyJ[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}\b')
PERSONAL_EMAIL = re.compile(rb'[A-Za-z0-9._%+-]+@(?:gmail|hotmail|outlook|yahoo)\.[A-Za-z.]+', re.I)
FORBIDDEN_SUFFIXES = {'.exe', '.pkg', '.dll', '.dylib', '.so', '.pem', '.key', '.pfx', '.p12', '.zip', '.gz'}
PRIVATE_NAMES = {'wallet.json', 'config.json', '.env'}
PRIVATE_PARTS = {'desktopid-linux-state', 'signing-tests', 'pkcs11-tests'}


def candidates(root):
    try:
        top = subprocess.run(['git', '-C', str(root), 'rev-parse', '--show-toplevel'],
                             capture_output=True, check=True, text=True)
        if Path(top.stdout.strip()).resolve() == root:
            result = subprocess.run(['git', '-C', str(root), 'ls-files', '--cached',
                                     '--others', '--exclude-standard', '-z'],
                                    capture_output=True, check=True)
            return sorted({os.fsdecode(name) for name in result.stdout.split(b'\0') if name})
    except (OSError, subprocess.CalledProcessError):
        pass
    names = []
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = [name for name in dirs if name not in GENERATED]
        for name in files:
            names.append((Path(directory) / name).relative_to(root).as_posix())
    return sorted(names)


def inspect(root, names):
    issues = []
    for name in names:
        relative = Path(name)
        file = root / relative
        # A deleted tracked file will not be included in the next snapshot.
        if not file.exists() and not file.is_symlink():
            continue
        if file.is_symlink() or not file.resolve().is_relative_to(root):
            issues.append((name, 'symbolic link or file outside project'))
            continue
        parts = set(relative.parts)
        if (relative.suffix.lower() in FORBIDDEN_SUFFIXES or relative.name in PRIVATE_NAMES
                or relative.name.startswith(('.env.', 'registration'))
                or parts & PRIVATE_PARTS or parts & {'analysis', 'tools', 'target', 'dist', 'test-results', '.local-research'}
                or 'desktopid-linux' in parts and '.config' in parts):
            issues.append((name, 'private state, binary or generated investigation artifact'))
            continue
        data = file.read_bytes()
        if PRIVATE_PEM.search(data):
            reviewed_fixture = (relative.as_posix() == LAB_FIXTURE
                                and hashlib.sha256(data.replace(b'\r\n', b'\n')).hexdigest() == LAB_SHA256)
            if not reviewed_fixture:
                issues.append((name, 'unreviewed private key'))
        if JWT.search(data):
            issues.append((name, 'possible authentication token'))
        if PERSONAL_EMAIL.search(data):
            issues.append((name, 'personal email address'))
    return issues


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    root = parser.parse_args().root.resolve(strict=True)
    names = candidates(root)
    issues = inspect(root, names)
    for name, reason in issues:
        print(f'{name}: {reason}; content omitted')
    print(f'Publication check: {len(names)} source files; {len(issues)} issue(s).')
    return 1 if issues else 0


if __name__ == '__main__':
    raise SystemExit(main())
