"""Reproducible source-only ZIP for transport or GitHub preparation."""
import argparse
import hashlib
from pathlib import Path
import zipfile

from scripts.check_publication import candidates, inspect


def build(destination):
    root = Path(__file__).resolve().parents[1]
    names = candidates(root)
    issues = inspect(root, names)
    if issues:
        raise RuntimeError('Publication check failed; run scripts/check_publication.py for safe details')
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name in names:
            file = root / name
            if not file.is_file():
                continue
            info = zipfile.ZipInfo('desktopid-linux/' + Path(name).as_posix(), (1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100755 if name.endswith('.sh') else 0o100644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, file.read_bytes())
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    destination.with_suffix(destination.suffix + '.sha256').write_text(
        f'{digest}  {destination.name}\n', encoding='utf-8')
    print(f'{destination.name}: {destination.stat().st_size:,} bytes; SHA256 {digest}; source files only')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('dist/desktopid-linux-source.zip'))
    build(parser.parse_args().output)
