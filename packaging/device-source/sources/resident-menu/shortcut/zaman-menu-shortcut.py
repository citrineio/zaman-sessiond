#!/usr/bin/env python3
"""Open the existing Quickshell preview on a dedicated InputPlumber signal.
Does not change interception modes, create targets, or close either menu.
Requires the system Python's existing PyGObject/Gio bindings.
"""
import errno
import math
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

SERVICE = 'org.shadowblip.InputPlumber'
ROOT = '/org/shadowblip/InputPlumber'
COMPOSITE = 'org.shadowblip.Input.CompositeDevice'
INTERFACE = 'org.shadowblip.Input.DBusDevice'
EVENT = 'zaman_new_menu'
INVENTORY_POLL_SECONDS = 30
INVENTORY_DEBOUNCE_MS = 100


class TriggerGuard:
    def __init__(self):
        self.pressed = set()
        self.last_launch = -math.inf

    def reset(self):
        self.pressed.clear()

    def accept(self, path, event, value, now):
        if event != EVENT or not isinstance(value, (int, float)) or not math.isfinite(value):
            return False
        if value <= 0.5:
            self.pressed.discard(path)
            return False
        if path in self.pressed:
            return False
        self.pressed.add(path)
        # USB and Bluetooth can emit the same physical chord. Do not launch twice.
        if now - self.last_launch < 1.0:
            return False
        self.last_launch = now
        return True


def preview_running(runtime):
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        probe.bind('\0zaman-menu-dev:' + runtime)
        return False
    except OSError as error:
        if error.errno == errno.EADDRINUSE:
            return True
        raise
    finally:
        probe.close()


def main():
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib

    runtime = os.environ.get('XDG_RUNTIME_DIR')
    if not runtime:
        raise RuntimeError('XDG_RUNTIME_DIR is missing; run as the graphical-session user.')
    listener_lock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener_lock.bind('\0zaman-menu-shortcut:' + runtime)
    home = Path.home()
    launcher = Path(os.environ.get('ZAMAN_MENU_LAUNCHER', home / 'zaman-quickshell-prototype/run.sh'))
    project = Path(os.environ.get('ZAMAN_SESSIOND_PROJECT', home / 'zaman-sessiond-publish'))
    if not launcher.is_file():
        raise RuntimeError(f'Existing launcher missing: {launcher}')

    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    guard = TriggerGuard()
    inventory = {'owner': None, 'targets': set(), 'warning': None}
    child = None
    loop = GLib.MainLoop()
    refresh_source = 0

    def refresh():
        try:
            owner = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (SERVICE,)),
                GLib.VariantType.new('(s)'), Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]
            objects = bus.call_sync(owner, ROOT, 'org.freedesktop.DBus.ObjectManager',
                'GetManagedObjects', None, None, Gio.DBusCallFlags.NONE, 1500, None).unpack()[0]
            confirmed = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (SERVICE,)),
                GLib.VariantType.new('(s)'), Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]
            if confirmed != owner:
                raise RuntimeError('Input service changed during inventory refresh')
            targets = set()
            for interfaces in objects.values():
                props = interfaces.get(COMPOSITE)
                if props is not None:
                    targets.update(props.get('DbusDevices', []))
            if owner != inventory['owner']:
                guard.reset()
            guard.pressed.intersection_update(targets)
            inventory.update(owner=owner, targets=targets, warning=None)
        except Exception as error:
            detail = str(error)
            if detail != inventory['warning']:
                print('Input service unavailable; waiting:', detail, flush=True)
            inventory.update(owner=None, targets=set(), warning=detail)
            guard.reset()
        return GLib.SOURCE_CONTINUE

    def refresh_once():
        nonlocal refresh_source
        refresh_source = 0
        refresh()
        return GLib.SOURCE_REMOVE

    def queue_refresh():
        nonlocal refresh_source
        # Device creation emits several signals. Coalesce one hotplug burst.
        if not refresh_source:
            refresh_source = GLib.timeout_add(INVENTORY_DEBOUNCE_MS, refresh_once)

    def refresh_now():
        nonlocal refresh_source
        if refresh_source:
            GLib.source_remove(refresh_source)
            refresh_source = 0
        refresh()

    def on_inventory_changed(connection, sender, path, interface, member, params, data):
        if inventory['owner'] is not None and sender != inventory['owner']:
            return
        queue_refresh()

    def on_properties_changed(connection, sender, path, interface, member, params, data):
        if sender != inventory['owner']:
            return
        changed_interface, changed, invalidated = params.unpack()
        if changed_interface == COMPOSITE and (
                {'DbusDevices', 'SourceDevicePaths'} & (set(changed) | set(invalidated))):
            queue_refresh()

    def on_owner_changed(connection, sender, path, interface, member, params, data):
        name, old, new = params.unpack()
        if name == SERVICE:
            inventory.update(owner=None, targets=set())
            guard.reset()
            queue_refresh()

    def on_input(connection, sender, path, interface, member, params, data):
        nonlocal child
        try:
            event, value = params.unpack()
            # Keep a defensive check in addition to the bus-side arg0 filter.
            if event != EVENT:
                return
            # A chord can beat the debounced hotplug update. Refresh only on
            # this rare menu event so the first chord after reconnect still works.
            if refresh_source or sender != inventory['owner'] or path not in inventory['targets']:
                refresh_now()
            if sender != inventory['owner'] or path not in inventory['targets']:
                return
            if not guard.accept(path, event, value, time.monotonic()):
                return
            if (child is not None and child.poll() is None) or preview_running(runtime):
                print('Modern menu already running; leaving it open.', flush=True)
                return
            status = session.call_sync('com.kawnelectro.Zaman.Session1', '/com/kawnelectro/Zaman/Session1',
                'com.kawnelectro.Zaman.Session1', 'ForegroundStatus', None, None,
                Gio.DBusCallFlags.NONE, 1500, None).unpack()[0]
            if status not in ('library', 'game', 'storefront'):
                print(f'Not launching over foreground owner: {status}', flush=True)
                return
            print('Start + Select: opening modern menu.', flush=True)
            child = subprocess.Popen(['sh', str(launcher), str(project)],
                cwd=str(home), start_new_session=True)
        except Exception as error:
            print('Menu shortcut failed:', error, flush=True)

    # Install subscriptions before the initial snapshot to close the setup gap.
    subscriptions = [bus.signal_subscribe(SERVICE, INTERFACE, 'InputEvent', None, EVENT,
        Gio.DBusSignalFlags.NONE, on_input, None)]
    for member in ('InterfacesAdded', 'InterfacesRemoved'):
        subscriptions.append(bus.signal_subscribe(SERVICE, 'org.freedesktop.DBus.ObjectManager',
            member, ROOT, None, Gio.DBusSignalFlags.NONE, on_inventory_changed, None))
    subscriptions.append(bus.signal_subscribe(SERVICE, 'org.freedesktop.DBus.Properties',
        'PropertiesChanged', None, COMPOSITE, Gio.DBusSignalFlags.NONE, on_properties_changed, None))
    subscriptions.append(bus.signal_subscribe('org.freedesktop.DBus', 'org.freedesktop.DBus',
        'NameOwnerChanged', '/org/freedesktop/DBus', SERVICE,
        Gio.DBusSignalFlags.NONE, on_owner_changed, None))
    refresh()
    fallback_source = GLib.timeout_add_seconds(INVENTORY_POLL_SECONDS, refresh)
    for signum in (signal.SIGINT, signal.SIGTERM):
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signum,
            lambda: (loop.quit(), GLib.SOURCE_REMOVE)[1])
    print('Listening for Start + Select (event-filtered; inventory fallback 30s). Guide remains assigned to the fallback.', flush=True)
    try:
        loop.run()
    finally:
        for subscription in subscriptions:
            bus.signal_unsubscribe(subscription)
        GLib.source_remove(fallback_source)
        if refresh_source:
            GLib.source_remove(refresh_source)
        listener_lock.close()
    # The launched menu owns its own lifecycle; stopping this listener does not close it.


if __name__ == '__main__':
    main()
