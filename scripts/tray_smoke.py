"""Test the real Rust tray on an isolated D-Bus session with a synthetic host.

Run with dbus-run-session. No real GUI, account, cloud request or NSS changes.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time

from gi.repository import Gio, GLib

WATCHER = 'org.kde.StatusNotifierWatcher'
ITEM = 'org.kde.StatusNotifierItem'
MENU = 'com.canonical.dbusmenu'
XML = '''<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
</interface></node>'''


def wait_until(condition, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(0.05)
    raise AssertionError('Timed out: ' + description)


def smoke(binary):
    # Refuse to replace the real desktop's watcher even if invoked incorrectly.
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    def call(destination, path, interface, method, parameters=None):
        return connection.call_sync(destination, path, interface, method, parameters,
                                    None, Gio.DBusCallFlags.NONE, 5000, None)
    def bus(method, parameters=None):
        return call('org.freedesktop.DBus', '/org/freedesktop/DBus',
                    'org.freedesktop.DBus', method, parameters)
    assert not bus('NameHasOwner', GLib.Variant('(s)', (WATCHER,))).unpack()[0], \
        'Use dbus-run-session; the real tray watcher must not be replaced'
    registered = []
    def method(_connection, sender, _path, _interface, name, parameters, invocation):
        if name == 'RegisterStatusNotifierItem':
            service = parameters.unpack()[0]
            registered.append(sender if service.startswith('/') else service)
        invocation.return_value(GLib.Variant('()', ()))
    def property_value(_connection, _sender, _path, _interface, name):
        return {'RegisteredStatusNotifierItems': GLib.Variant('as', registered),
                'IsStatusNotifierHostRegistered': GLib.Variant('b', True),
                'ProtocolVersion': GLib.Variant('i', 0)}[name]
    registration = connection.register_object('/StatusNotifierWatcher',
        Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], method, property_value, None)
    loop = GLib.MainLoop()
    thread = threading.Thread(target=loop.run, daemon=True)
    thread.start()
    processes = []
    try:
        with tempfile.TemporaryDirectory(prefix='axia-tray-lab-') as temporary:
            home = Path(temporary)
            config = home/'config'
            state = config/'desktopid-linux'
            marker = home/'gui-opened'
            helper = home/'.local/libexec/desktopid-linux/desktopid-client'
            helper.parent.mkdir(parents=True)
            helper.write_text('#!/usr/bin/python3\nimport time\nfrom pathlib import Path\n'
                              f'with Path({str(marker)!r}).open("a") as output: output.write("open\\n")\n'
                              'time.sleep(1.5)\n')
            helper.chmod(0o700)
            environment = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(config))
            command = [str(binary), '--state-dir', str(state)]
            def cli(*arguments):
                return subprocess.run([*command, *arguments], env=environment,
                                      capture_output=True, text=True, timeout=10, check=True)
            cli('autostart', '--enable')
            preferences = config/'axia-remotedid/preferences.json'
            before = preferences.read_bytes()
            resident = subprocess.Popen([*command, 'startup'], env=environment,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            processes.append(resident)
            def item_names():
                return [name for name in bus('ListNames').unpack()[0]
                        if name.startswith('org.kde.StatusNotifierItem-')]
            wait_until(lambda: len(item_names()) == 1, 'resident without a watcher')
            assert resident.poll() is None and not marker.exists() and not state.exists()
            assert preferences.read_bytes() == before
            # The host appears after autostart: the resident must register then.
            bus('RequestName', GLib.Variant('(su)', (WATCHER, 0)))
            wait_until(lambda: len(registered) == 1, 'late tray host registration')
            destination = registered[0]
            properties = call(destination, '/StatusNotifierItem', 'org.freedesktop.DBus.Properties',
                              'GetAll', GLib.Variant('(s)', (ITEM,))).unpack()[0]
            assert properties['Id'] == 'axia-remotedid'
            assert properties['Title'] == 'Axia RemotedID'
            assert properties['WindowId'] == 0
            cli('startup')
            assert len(item_names()) == 1, 'Duplicate resident registered'
            assert not marker.exists() and not state.exists()
            # Shell/extension restart must re-register the same resident.
            bus('ReleaseName', GLib.Variant('(s)', (WATCHER,)))
            time.sleep(0.3)
            bus('RequestName', GLib.Variant('(su)', (WATCHER, 0)))
            wait_until(lambda: len(registered) == 2, 'host restart registration')
            assert len(item_names()) == 1
            menu_path = properties['Menu']
            def layout():
                return call(destination, menu_path, MENU, 'GetLayout',
                            GLib.Variant('(iias)', (0, -1, []))).unpack()[1]
            def entries():
                return {entry[1].get('label'): entry for entry in layout()[2]}
            def click(label):
                entry = entries()[label]
                call(destination, menu_path, MENU, 'Event',
                     GLib.Variant('(isvu)', (entry[0], 'clicked', GLib.Variant('i', 0), 0)))
            call(destination, '/StatusNotifierItem', ITEM, 'Activate', GLib.Variant('(ii)', (0, 0)))
            wait_until(marker.exists, 'explicit GUI activation')
            click('Abrir Axia RemotedID')
            time.sleep(0.2)
            assert marker.read_text().splitlines() == ['open'], 'Repeated click opened another GUI'
            time.sleep(1.6)
            click('Abrir Axia RemotedID')
            wait_until(lambda: len(marker.read_text().splitlines()) == 2, 'reopen after closing GUI')
            time.sleep(1.6)
            assert resident.poll() is None, 'Closing GUI stopped the tray'
            click('Iniciar com o sistema')
            wait_until(lambda: not json.loads(preferences.read_text())['autostart_enabled'],
                       'disable autostart in menu')
            wait_until(lambda: entries()['Iniciar com o sistema'][1]['toggle-state'] == 0,
                       'menu checkbox refresh')
            click('Iniciar com o sistema')
            wait_until(lambda: json.loads(preferences.read_text())['autostart_enabled'],
                       'enable autostart in menu')
            click('Sair')
            assert resident.wait(timeout=10) == 0
            stdout, stderr = resident.communicate()
            assert not stdout and not stderr, (stdout, stderr)
            wait_until(lambda: not item_names(), 'tray removal on exit')
            assert not state.exists()
            assert json.loads(preferences.read_text())['autostart_enabled']
            # File remains but the kernel lock must be released on exit.
            restarted = subprocess.Popen([*command, 'startup'], env=environment,
                                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            processes.append(restarted)
            wait_until(lambda: len(registered) == 3, 'resident restart after exit')
            destination = registered[-1]
            click('Sair')
            assert restarted.wait(timeout=10) == 0
            assert not state.exists()
        print('Tray: silent startup, late/restarted host, single resident, no window, '
              'explicit open/reopen, autostart toggle and exit passed; no real account')
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
        bus('ReleaseName', GLib.Variant('(s)', (WATCHER,)))
        connection.unregister_object(registration)
        loop.quit()
        thread.join(timeout=5)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    smoke(args.binary.resolve(strict=True))
