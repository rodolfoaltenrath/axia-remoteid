#!/usr/bin/python3
"""Desktop entry point for the system RPM; all account actions remain local UI."""
import argparse
import configparser
import os
from pathlib import Path
import subprocess
import sys
import tempfile

if __package__:
    from .firefox_pkcs11 import default_profile, native_profiles, register, registered_library
else:
    from firefox_pkcs11 import default_profile, native_profiles, register, registered_library

PREFIX = Path(__file__).resolve().parents[2]
HELPER = PREFIX/'libexec/axia-remotedid/desktopid-client'
MODULE = PREFIX/'lib64/axia-remotedid/libdesktopid_pkcs11.so'


def dialog(kind, text, *options):
    return subprocess.run(['/usr/bin/zenity', kind,
                           '--title=Axia RemotedID — Certificado em nuvem',
                           '--width=520', '--no-markup', '--text='+text, *options],
                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL).returncode


def state_directory():
    return Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home()/'.config')))/'desktopid-linux'


def migrate_user_shortcuts():
    home = Path.home().resolve()
    legacy_helper = home/'.local/libexec/desktopid-linux/desktopid-client'
    argument = str(legacy_helper).replace('\\', '\\\\\\\\').replace('%', '%%')
    for character in ('"', '`', '$'):
        argument = argument.replace(character, '\\\\'+character)
    applications = home/'.local/share/applications'
    if applications.is_dir() and applications.resolve().is_relative_to(home):
        for name in ['desktopid-linux-alpha.desktop', 'axia-remotedid-alpha.desktop']:
            entry = applications/name
            if not entry.is_file() or entry.is_symlink() or entry.stat().st_uid != os.geteuid():
                continue
            parser = configparser.ConfigParser(interpolation=None)
            parser.optionxform = str
            parser.read(entry)
            values = parser['Desktop Entry'] if parser.has_section('Desktop Entry') else {}
            if values.get('Name') not in ['DesktopID Linux Alpha', 'Axia RemotedID Alpha']:
                continue
            if values.get('Exec') != f'"{argument}" gui' or values.get('Hidden') == 'true':
                continue
            parser.set('Desktop Entry', 'Hidden', 'true')
            with tempfile.NamedTemporaryFile(mode='w', dir=applications, delete=False) as output:
                os.chmod(output.name, 0o644)
                parser.write(output)
                temporary = Path(output.name)
            temporary.replace(entry)
    command = home/'.local/bin/axia-remotedid'
    if (command.is_symlink() and command.lstat().st_uid == os.geteuid()
            and command.resolve() == legacy_helper.resolve()
            and command.parent.resolve().is_relative_to(home)):
        temporary = command.with_name('.axia-remotedid-rpm.pending')
        temporary.symlink_to(PREFIX/'bin/axia-remotedid')
        temporary.replace(command)


def select_profile():
    profiles = native_profiles(Path.home())
    if not profiles:
        raise RuntimeError('Abra e feche o Firefox nativo uma vez para criar seu perfil.')
    if len(profiles) == 1:
        return profiles[0]
    command = ['/usr/bin/zenity', '--list', '--title=Axia RemotedID — Perfil Firefox',
               '--text=Escolha o perfil do Firefox em que deseja usar seus certificados.',
               '--column=Perfil', '--column=Diretório', '--hide-column=2', '--print-column=2',
               '--ok-label=Selecionar', '--cancel-label=Cancelar']
    for index, profile in enumerate(profiles, 1):
        command += [f'{index}: {profile.name.split(".", 1)[-1]}', str(profile)]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode == 1:
        return None
    if result.returncode != 0 or Path(result.stdout.strip()) not in profiles:
        raise RuntimeError('Não foi possível selecionar o perfil do Firefox.')
    return Path(result.stdout.strip())


def setup_firefox(profile=None, ask=True):
    if not (state_directory()/'wallet.json').is_file():
        dialog('--info', 'Conecte sua conta e atualize os certificados antes de integrar o Firefox.')
        return 1
    if ask and dialog('--question',
                      'Habilitar seus certificados no Firefox?\n\nFeche todas as janelas do navegador antes de continuar.',
                      '--ok-label=Integrar Firefox', '--cancel-label=Agora não') != 0:
        return 0
    try:
        selected = Path(profile) if profile else select_profile()
        if selected is None:
            return 0
        register(selected, MODULE, Path.home(), migrate=True)
    except Exception as error:
        dialog('--error', 'Não foi possível integrar o Firefox.\n\n'+str(error)+
               '\n\nSe ainda não utilizou o Firefox nativo, abra e feche o navegador uma vez. '
               'Você pode tentar novamente pela opção “Integrar Firefox” do aplicativo.')
        return 1
    dialog('--info', 'Firefox integrado.\n\nReabra o navegador e escolha a entrada com certificado digital no serviço desejado.')
    return 0


def main(arguments):
    if os.geteuid() == 0:
        print('Abra o Axia RemotedID como seu usuário normal, sem sudo.', file=sys.stderr)
        return 1
    if arguments and arguments[0] == 'setup-firefox':
        parser = argparse.ArgumentParser(prog='axia-remotedid setup-firefox')
        parser.add_argument('--profile', type=Path)
        selected = parser.parse_args(arguments[1:])
        return setup_firefox(selected.profile)
    if arguments in ([], ['gui']):
        try:
            migrate_user_shortcuts()
        except (OSError, configparser.Error):
            # An unrelated/customized old shortcut must not prevent opening.
            pass
    result = subprocess.run([str(HELPER), *arguments]).returncode
    # Offer browser integration only after an explicit graphical connection.
    # CLI and session-startup calls never update Firefox databases.
    if result == 0 and arguments in ([], ['gui']) and (state_directory()/'wallet.json').is_file():
        try:
            integrated = registered_library(default_profile(Path.home())) == str(MODULE)
        except (OSError, RuntimeError):
            integrated = False
        if not integrated:
            setup_firefox()
    return result


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
