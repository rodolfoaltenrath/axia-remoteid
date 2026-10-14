"""RPM desktop workflow: cancellation, CLI isolation and profile migration."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import os

from scripts import desktop_app
from scripts.firefox_pkcs11 import register


class RpmDesktopTests(unittest.TestCase):
    def test_rpm_hides_only_its_legacy_shortcut_and_updates_its_own_alias(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            applications = home/'.local/share/applications'
            applications.mkdir(parents=True)
            legacy = home/'.local/libexec/desktopid-linux/desktopid-client'
            entry = applications/'axia-remotedid-alpha.desktop'
            entry.write_text(f'[Desktop Entry]\nName=Axia RemotedID Alpha\nExec="{legacy}" gui\n')
            foreign = applications/'desktopid-linux-alpha.desktop'
            foreign.write_text('[Desktop Entry]\nName=DesktopID Linux Alpha\nExec=/usr/bin/other\n')
            original_foreign = foreign.read_bytes()
            alias = home/'.local/bin/axia-remotedid'
            alias.parent.mkdir(parents=True)
            alias.symlink_to(legacy)
            with patch.dict(os.environ, {'HOME': str(home)}), \
                    patch.object(desktop_app, 'PREFIX', Path('/usr')):
                desktop_app.migrate_user_shortcuts()
            self.assertIn('Hidden = true', entry.read_text())
            self.assertEqual(foreign.read_bytes(), original_foreign)
            self.assertEqual(alias.readlink(), Path('/usr/bin/axia-remotedid'))

    def test_cancel_browser_setup_does_not_change_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            (state/'wallet.json').write_text('{}')
            with patch.object(desktop_app, 'state_directory', return_value=state), \
                    patch.object(desktop_app, 'dialog', return_value=1), \
                    patch.object(desktop_app, 'register') as registration:
                self.assertEqual(desktop_app.setup_firefox(), 0)
                registration.assert_not_called()

    def test_browser_setup_without_account_does_not_register(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(desktop_app, 'state_directory', return_value=Path(directory)), \
                    patch.object(desktop_app, 'dialog') as dialog, \
                    patch.object(desktop_app, 'register') as registration:
                self.assertEqual(desktop_app.setup_firefox(), 1)
                registration.assert_not_called()
                self.assertIn('Conecte sua conta', dialog.call_args.args[1])

    def test_cli_does_not_offer_or_configure_firefox(self):
        with patch.object(desktop_app.os, 'geteuid', return_value=1000), \
                patch.object(desktop_app.subprocess, 'run') as command, \
                patch.object(desktop_app, 'setup_firefox') as setup:
            command.return_value.returncode = 0
            self.assertEqual(desktop_app.main(['--version']), 0)
            setup.assert_not_called()

    def test_root_cannot_create_a_desktop_account(self):
        with patch.object(desktop_app.os, 'geteuid', return_value=0), \
                patch.object(desktop_app.subprocess, 'run') as command:
            self.assertEqual(desktop_app.main(['gui']), 1)
            command.assert_not_called()

    def test_legacy_module_migration_is_explicit_and_backed_up(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            profile = home/'profile'
            profile.mkdir()
            for name in ['pkcs11.txt', 'cert9.db', 'key4.db']:
                (profile/name).write_bytes(b'laboratory')
            legacy = home/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'
            legacy.parent.mkdir(parents=True)
            legacy.write_bytes(b'laboratory')
            legacy.chmod(0o700)
            state = {'library': str(legacy)}
            target = Path('/usr/lib64/axia-remotedid/libdesktopid_pkcs11.so')
            with patch('scripts.firefox_pkcs11.protected_system_module', return_value=True), \
                    patch('scripts.firefox_pkcs11.registered_library', side_effect=lambda _: state['library']), \
                    patch('scripts.firefox_pkcs11.NssDatabase') as nss:
                database = nss.return_value.__enter__.return_value
                database.certificate_count.return_value = 1
                database.add.side_effect = lambda module: state.update(library=str(module))
                with self.assertRaisesRegex(RuntimeError, 'was preserved'):
                    register(profile, target, home)
                database.remove.assert_not_called()
                result = register(profile, target, home, migrate=True)
                database.remove.assert_called_once()
                self.assertTrue(result['migrated_legacy_module'])
                backup = Path(result['backup_directory'])
                self.assertEqual((backup/'cert9.db').read_bytes(), b'laboratory')
                self.assertEqual(backup.stat().st_mode & 0o777, 0o700)
