"""Profile selection, browser locking and native desktop-launcher checks."""
import configparser
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from scripts.firefox_pkcs11 import default_profile, profile_lock
from scripts.install_alpha import install_launcher


class FirefoxIntegrationTests(unittest.TestCase):
    def test_install_default_overrides_legacy_profile_default(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            root = home/'.config/mozilla/firefox'
            (root/'release').mkdir(parents=True)
            (root/'legacy').mkdir()
            (root/'profiles.ini').write_text('[Profile0]\nPath=legacy\nDefault=1\n'
                                             '[InstallABC]\nDefault=release\nLocked=1\n')
            with patch.dict(os.environ, {'XDG_CONFIG_HOME': str(home/'.config')}):
                self.assertEqual(default_profile(home), root/'release')

    def test_multiple_install_profiles_require_explicit_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            root = home/'.config/mozilla/firefox'
            (root/'first').mkdir(parents=True)
            (root/'second').mkdir()
            (root/'profiles.ini').write_text('[InstallA]\nDefault=first\n'
                                             '[InstallB]\nDefault=second\n')
            with patch.dict(os.environ, {'XDG_CONFIG_HOME': str(home/'.config')}):
                with self.assertRaisesRegex(RuntimeError, 'pass --profile'):
                    default_profile(home)

    def test_lock_held_by_another_process_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            source = ('import fcntl, sys\n'
                      'with open(sys.argv[1], "w") as lock:\n'
                      ' fcntl.lockf(lock, fcntl.LOCK_EX)\n'
                      ' print("locked", flush=True)\n'
                      ' sys.stdin.readline()\n')
            child = subprocess.Popen([sys.executable, '-c', source, str(profile/'.parentlock')],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            try:
                self.assertEqual(child.stdout.readline().strip(), 'locked')
                with self.assertRaisesRegex(RuntimeError, 'Feche o Firefox'):
                    with profile_lock(profile):
                        self.fail('Must not acquire a live Firefox profile')
                self.assertIsNone(child.poll())
            finally:
                child.communicate('\n', timeout=5)
            with profile_lock(profile):
                self.assertTrue((profile/'.parentlock').is_file())

    def test_profile_lock_refuses_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            target = profile/'unrelated'
            target.write_text('preserve')
            (profile/'.parentlock').symlink_to(target)
            with self.assertRaises(OSError):
                with profile_lock(profile):
                    self.fail('Must not follow a lock symlink')
            self.assertEqual(target.read_text(), 'preserve')

    def test_launcher_supports_spaces_and_exec_metacharacters(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)/'home space $value %f'
            home.mkdir()
            launcher = install_launcher(home)
            parser = configparser.ConfigParser(interpolation=None)
            parser.read(launcher)
            self.assertEqual(parser['Desktop Entry']['Terminal'], 'false')
            self.assertIn('Axia RemotedID Alpha', parser['Desktop Entry']['Name'])
            if shutil.which('desktop-file-validate'):
                subprocess.run(['desktop-file-validate', str(launcher)], check=True,
                               capture_output=True, text=True, timeout=10)

    def test_launcher_refuses_to_replace_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            applications = home/'.local/share/applications'
            applications.mkdir(parents=True)
            target = home/'unrelated'
            target.write_text('preserve')
            (applications/'axia-remotedid-alpha.desktop').symlink_to(target)
            with self.assertRaisesRegex(RuntimeError, 'foreign launcher'):
                install_launcher(home)
            self.assertEqual(target.read_text(), 'preserve')
