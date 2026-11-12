#!/usr/bin/env python3
"""Read-only controller/network/Bluetooth/battery events. No interception or writes."""
import concurrent.futures
import json
import hashlib
import os
from pathlib import Path
import queue
import re
import socket
import sys
import threading
import time
import uuid


def label(value):
    return re.sub(r'[\x00-\x1f\x7f]', '', str(value or ''))[:120]


def network_devices(paths, properties):
    """Read public active-profile/AP metadata, never credentials or settings."""
    values = {}
    def optional(path, interface):
        if not path or path == '/':
            return {}
        try:
            return properties(path, interface)
        except Exception:
            return {}  # AP/profile paths can vanish while switching networks.
    for path in paths[:16]:
        props = properties(path, 'org.freedesktop.NetworkManager.Device')
        kind = props.get('DeviceType')
        if kind not in (1, 2):
            continue
        iface = label(props.get('Interface', path))
        profile = optional(props.get('ActiveConnection'), 'org.freedesktop.NetworkManager.Connection.Active')
        ssid = ''
        ssid_identity = ''
        if kind == 2:
            wifi = optional(path, 'org.freedesktop.NetworkManager.Device.Wireless')
            ap = optional(wifi.get('ActiveAccessPoint'), 'org.freedesktop.NetworkManager.AccessPoint')
            raw = ap.get('Ssid', [])
            try:
                raw = bytes(raw)
                ssid = label(raw.decode('utf-8', errors='replace'))
                ssid_identity = hashlib.sha256(raw).hexdigest() if raw else ''
            except (ValueError, TypeError):
                pass
        profile_name = label(profile.get('Id'))
        values[iface] = dict(name=ssid or profile_name or ('Wi-Fi' if kind == 2 else 'Ethernet') + ' · ' + iface,
            named=bool(ssid or profile_name), networkName=ssid_identity,
            connection=label(profile.get('Uuid')) or profile_name, state=int(props.get('State', 0)))
    return values


def bluetooth_adapters(objects):
    return {str(props.get('Address') or path): (label(props.get('Alias') or props.get('Name')) or 'Bluetooth adapter', props['Powered'])
        for path, interfaces in objects.items() if (props := interfaces.get('org.bluez.Adapter1')) is not None
        and isinstance(props.get('Powered'), bool)}


class ScanScheduler:
    """Keep one worker; a signal during a scan requests one follow-up snapshot."""
    def __init__(self, executor, scan, post):
        self.executor, self.scan, self.post = executor, scan, post
        self.active, self.pending = None, False
    def request(self):
        if self.active is not None and not self.active.done():
            self.pending = True
        else:
            self.pending = False
            self.active = self.executor.submit(self.scan)
            self.active.add_done_callback(lambda future: self.post(self.finished))
        return True
    def finished(self):
        if self.pending:
            self.request()
        return False


class Changes:
    """Do not turn discovery, bus failure or owner restart into hotplug events."""
    def __init__(self):
        self.previous = {}
        self.battery_armed = {}
        self.last_sent = {}
        self.generation = uuid.uuid4().hex[:12]
        self.sequence = 0

    def update(self, kind, owner, values):
        old = self.previous.get(kind)
        if kind == 'network' and old is not None and old[0] == owner:
            normalized = {}
            for identity, raw in values.items():
                value = dict(raw)
                previous = old[1].get(identity)
                if value['state'] not in (100, 120) and value['state'] > 30:
                    # Preserve the last terminal connection identity as well as state.
                    value = dict(previous or value, state=previous['state'] if previous else 0)
                    value['attemptName'] = raw['name'] if raw.get('named') else value.get('attemptName', '')
                elif previous:
                    if value['state'] <= 30 and previous['state'] == 100:
                        value['name'] = previous['name']
                    elif value['state'] == 120 and not value.get('named'):
                        value['name'] = previous.get('attemptName') or previous['name']
                    elif value['state'] == 100 and previous['state'] == 100:
                        same_profile = not value.get('connection') or value.get('connection') == previous.get('connection')
                        if not value.get('named') or (not value.get('networkName') and previous.get('networkName') and same_profile):
                            value.update(name=previous['name'], networkName=previous.get('networkName', ''))
                        if not value.get('connection'):
                            value['connection'] = previous.get('connection', '')
                normalized[identity] = value
            values = normalized
        self.previous[kind] = (owner, values)
        if old is None or old[0] != owner:
            return []
        before = old[1]
        events = []
        if kind == 'controllers':
            for identity in sorted(set(values) - set(before)):
                events.append(dict(key='input:' + identity + ':connected', title='Controller connected',
                    body=values[identity], category='input', state='connected'))
            for identity in sorted(set(before) - set(values)):
                events.append(dict(key='input:' + identity + ':disconnected', title='Controller disconnected',
                    body=before[identity], category='input', state='disconnected'))
        elif kind == 'network':
            for identity, value in values.items():
                name, state = value['name'], value['state']
                previous = before.get(identity)
                if previous is None:
                    continue
                changed = state == previous['state'] == 100 and any(previous.get(key) and value.get(key)
                    and previous[key] != value[key] for key in ('connection', 'networkName'))
                if state == 100 and (previous['state'] != 100 or changed):
                    events.append(dict(key='network:' + identity + ':connected', title='Network changed' if changed else 'Network connected', body=name, category='network', state='connected'))
                elif state == 120 and previous['state'] != 120:
                    events.append(dict(key='network:' + identity + ':failed', title='Connection failed', body=name, category='network', state='error'))
                elif previous['state'] == 100 and state <= 30:
                    events.append(dict(key='network:' + identity + ':disconnected', title='Network disconnected', body=name, category='network', state='disconnected'))
        elif kind == 'bluetooth':
            for identity, (name, powered) in values.items():
                previous = before.get(identity)
                if previous is not None and previous[1] != powered:
                    events.append(dict(key='bluetooth:' + identity, title='Bluetooth on' if powered else 'Bluetooth off',
                        body=name, category='bluetooth', state='enabled' if powered else 'disabled'))
        elif kind == 'battery':
            for identity, (name, percent) in values.items():
                if identity not in before:
                    self.battery_armed[identity] = percent > 20
                    continue
                armed = self.battery_armed.get(identity, before[identity][1] > 20)
                if percent >= 30:
                    self.battery_armed[identity] = True
                elif percent <= 20 and armed:
                    self.battery_armed[identity] = False
                    events.append(dict(key='battery:' + identity + ':low', title='Controller battery low', body=f'{name} · {percent}%', category='battery', state='warning'))
        now = time.monotonic()
        filtered = []
        for event in events:
            if kind in ('network', 'bluetooth'):
                # Distinct confirmed transitions must survive rapid off/on or A/B/A.
                # Stable snapshots generate no event; repeated delivery keeps this ID.
                self.sequence += 1
                event['key'] += ':' + self.generation + ':' + str(self.sequence)
                filtered.append(event)
                continue
            cooldown = 900 if event['category'] == 'battery' else 5
            if now - self.last_sent.get(event['key'], -cooldown) >= cooldown:
                self.last_sent[event['key']] = now
                filtered.append(event)
        return filtered


def main():
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib
    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    changes = Changes()
    outgoing = queue.Queue(maxsize=32)
    def deliver():
        while True:
            event = outgoing.get()
            try:
                path = Path(os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid()))) / 'zaman-notifications/control.sock'
                with socket.socket(socket.AF_UNIX) as sock:
                    sock.settimeout(0.5)
                    sock.connect(str(path))
                    sock.sendall((json.dumps(dict(action='event', source='Zaman', **event)) + '\n').encode())
                    sock.recv(4096)
            except OSError:
                pass  # No replay of stale system events after service recovery.
    threading.Thread(target=deliver, daemon=True).start()
    def call(service, path, interface, method, signature='()', args=()):
        return bus.call_sync(service, path, interface, method, GLib.Variant(signature, args), None,
            Gio.DBusCallFlags.NO_AUTO_START, 1000, None).unpack()[0]
    def owner(service):
        return call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetNameOwner', '(s)', (service,))
    def managed(service, root):
        current = owner(service)
        objects = call(current, root, 'org.freedesktop.DBus.ObjectManager', 'GetManagedObjects')
        if owner(service) != current:
            raise ValueError('service changed during snapshot')
        return current, objects
    def controllers():
        current, objects = managed('org.shadowblip.InputPlumber', '/org/shadowblip/InputPlumber')
        values = {}
        for interfaces in objects.values():
            props = interfaces.get('org.shadowblip.Input.CompositeDevice')
            if props is None:
                continue
            sources = sorted(set(props.get('SourceDevicePaths', [])))
            # Composite logical identity; never enumerate virtual target pads as physical devices.
            if sources:
                values[hashlib.sha256(json.dumps(sources, separators=(',', ':')).encode()).hexdigest()] = str(props.get('Name', 'Controller'))[:80]
        return current, values
    def networks():
        service = 'org.freedesktop.NetworkManager'
        current = owner(service)
        paths = call(current, '/org/freedesktop/NetworkManager', service, 'GetDevices')
        values = network_devices(paths, lambda path, interface: call(current, path,
            'org.freedesktop.DBus.Properties', 'GetAll', '(s)', (interface,)))
        if owner(service) != current:
            raise ValueError('network service changed during snapshot')
        return current, values
    def bluetooth():
        current, objects = managed('org.bluez', '/')
        return current, bluetooth_adapters(objects)
    def batteries():
        current, objects = managed('org.bluez', '/')
        values = {}
        for interfaces in objects.values():
            device, battery = interfaces.get('org.bluez.Device1', {}), interfaces.get('org.bluez.Battery1', {})
            # Only connected Bluetooth HID devices with explicit telemetry.
            uuids = [str(u).lower() for u in device.get('UUIDs', [])]
            device_class = int(device.get('Class', 0))
            # Bluetooth CoD peripheral minor: joystick=1, gamepad=2; LE HID appearances 0x3c3/0x3c4.
            gamepad = (device_class & 0x1f00) == 0x0500 and ((device_class >> 2) & 0x0f) in (1, 2)
            gamepad = gamepad or int(device.get('Appearance', 0)) in (0x03c3, 0x03c4)
            if gamepad and device.get('Connected') and any(u.startswith(('00001124-', '00001812-')) for u in uuids) and 'Percentage' in battery:
                values[str(device.get('Address', ''))] = (str(device.get('Alias', 'Controller'))[:80], int(battery['Percentage']))
        return current, values
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    failures = {}
    def scan():
        health = {}
        for kind, reader in [('controllers', controllers), ('network', networks), ('bluetooth', bluetooth), ('battery', batteries)]:
            try:
                current, values = reader()
                health[kind] = {'available':True, 'entities':len(values)}
                if kind in failures:
                    print('Notification source recovered: '+kind, flush=True)
                    failures.pop(kind,None)
                for event in changes.update(kind, current, values):
                    try:
                        outgoing.put_nowait(event)
                    except queue.Full:
                        break
            except Exception as error:
                message = str(error)[:400]
                health[kind] = {'available':False, 'error':message}
                if failures.get(kind) != message:
                    print('Notification source unavailable: '+kind+': '+message,file=sys.stderr,flush=True)
                    failures[kind] = message
                # Rebaseline only this source on recovery. Never synthesize disconnects from failures.
                changes.previous.pop(kind, None)
        try:
            status = Path(os.environ.get('XDG_RUNTIME_DIR', '/run/user/'+str(os.getuid()))) / 'zaman-notification-events.json'
            temporary = status.with_suffix('.tmp')
            temporary.write_text(json.dumps({'timestamp':time.time(), 'sources':health}))
            temporary.replace(status)
        except OSError as error:
            print('Could not write source diagnostics: '+str(error),file=sys.stderr,flush=True)
    scheduler = ScanScheduler(executor, scan, lambda callback: GLib.idle_add(callback))
    schedule = scheduler.request
    # Bounded coalescing: continuous signal/strength changes cannot starve scans.
    def invalidated(*args):
        nonlocal debounce
        if debounce:
            return
        def settled():
            nonlocal debounce
            debounce = 0
            schedule()
            return False
        debounce = GLib.timeout_add(200, settled)
    debounce = 0
    for service in ['org.shadowblip.InputPlumber', 'org.freedesktop.NetworkManager', 'org.bluez']:
        bus.signal_subscribe(service, None, None, None, None, Gio.DBusSignalFlags.NONE, invalidated)
    GLib.timeout_add_seconds(15, schedule)
    schedule()
    print('Zaman event observer ready (read-only, startup snapshots suppressed)', flush=True)
    GLib.MainLoop().run()


if __name__ == '__main__':
    try:
        main()
    except (ImportError, OSError) as error:
        print('ERROR: system event observer unavailable: ' + str(error), file=sys.stderr)
        sys.exit(1)
