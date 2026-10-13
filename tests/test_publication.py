"""Regressions for keeping account material out of a public source snapshot."""
import base64
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from scripts.check_publication import LAB_FIXTURE, candidates, inspect


class PublicationTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('git'), 'Requires Git')
    def test_gitignore_cannot_hide_an_already_tracked_private_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(['git', 'init', '-q', str(root)], check=True, capture_output=True)
            (root / '.gitignore').write_text('wallet.json\n')
            (root / 'wallet.json').write_text('{}')
            subprocess.run(['git', '-C', str(root), 'add', '-f', 'wallet.json'],
                           check=True, capture_output=True)
            names = candidates(root)
            self.assertIn('wallet.json', names)
            self.assertTrue(inspect(root, names))

    def test_state_is_rejected_even_without_recognizable_secret(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'wallet.json').write_text('{}')
            self.assertTrue(inspect(root, ['wallet.json']))

    def test_public_fixture_is_allowed_but_copy_or_modified_fixture_is_not(self):
        project = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = root / LAB_FIXTURE
            fixture.parent.mkdir(parents=True)
            shutil.copyfile(project / LAB_FIXTURE, fixture)
            self.assertEqual(inspect(root, [LAB_FIXTURE]), [])
            fixture.write_bytes(fixture.read_bytes().replace(b'\r\n', b'\n'))
            self.assertEqual(inspect(root, [LAB_FIXTURE]), [])
            shutil.copyfile(fixture, root / 'copied-key.json')
            self.assertTrue(inspect(root, ['copied-key.json']))
            fixture.write_bytes(fixture.read_bytes() + b'\n')
            self.assertTrue(inspect(root, [LAB_FIXTURE]))

    def test_token_is_detected_without_exposing_its_value(self):
        def encode(value):
            return base64.urlsafe_b64encode(value).rstrip(b'=')
        token = b'.'.join([encode(b'{"alg":"HS256","typ":"JWT"}'),
                           encode(b'{"sub":"synthetic laboratory account"}'),
                           encode(b'laboratory signature bytes only')])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'accidental.txt').write_bytes(token)
            issues = inspect(root, ['accidental.txt'])
            self.assertEqual(len(issues), 1)
            self.assertNotIn(token.decode(), str(issues))
