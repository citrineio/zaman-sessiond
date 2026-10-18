#!/usr/bin/env python3
"""Steam process/window evidence and existing Zaman storefront API.

Uses the accepted managed Steam startup, then sends an App ID to the native
client. Return is a conservative experimental check, not a complete Play API.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import sys
import time
import tomllib
import uuid

SERVICE = 'com.kawnelectro.Zaman.Session1'
OBJECT = '/com/kawnelectro/Zaman/Session1'
UNIT = 'zaman-store-steam.service'
BUSY = {'Starting', 'Running', 'Stopping', 'CleanupFailed'}


def valid_app(value):
    if not re.fullmatch(r'[1-9][0-9]{0,9}', str(value)) or int(value) > 4294967295:
        raise ValueError('App ID must be a canonical positive uint32')
    return str(value)


def game_class(value):
    match = re.fullmatch(r'steam_app_([1-9][0-9]{0,9})', value or '')
    if match:
        try:
            return valid_app(match[1])
        except ValueError:
            pass
    return None


def env_app(content):
    # Never retain or emit the rest of a process environment.
    for field in content.split(b'\0'):
        if field.startswith(b'SteamAppId='):
            try:
                return valid_app(field.split(b'=', 1)[1].decode('ascii'))
            except (UnicodeError, ValueError):
                return None
    return None


def parse_stat(content):
    pid, rest = content.split(' (', 1)
    comm, fields = rest.rsplit(')', 1)
    fields = fields.split()
    return {'pid': int(pid), 'comm': comm, 'ppid': int(fields[1]),
            'start': int(fields[19]), 'state': fields[0]}


def in_group(actual, expected):
    return bool(expected and expected != '/' and
                (actual == expected or actual.startswith(expected + '/')))


def process_group(pid):
    for line in Path(f'/proc/{pid}/cgroup').read_text().splitlines():
        if line.startswith('0::'):
            return line[3:]
    raise RuntimeError('Unified cgroup membership unavailable')


def process_row(pid):
    return parse_stat(Path(f'/proc/{pid}/stat').read_text())


def window_rows(tree):
    result = []
    def walk(node, workspace=''):
        if node.get('type') == 'workspace':
            workspace = node.get('name', '')
        if node.get('pid'):
            props = node.get('window_properties') or {}
            cls = props.get('class')
            app_id = node.get('app_id')
            result.append({'id': node['id'], 'pid': node['pid'],
                           'class': cls, 'app_id': app_id,
                           'app': game_class(cls) or game_class(app_id),
                           'workspace': workspace, 'focused': bool(node.get('focused'))})
        for child in node.get('nodes', []) + node.get('floating_nodes', []):
            walk(child, workspace)
    walk(tree)
    return result


def client_visible(windows):
    return any(not w['app'] and w['focused'] and w['workspace'] == 'store-steam'
               and ((w['class'] or '').lower() == 'steam'
                    or (w['app_id'] or '').lower() in ('steam', 'com.valvesoftware.steam'))
               for w in windows)


class Evidence:
    """Retain PID+start-time identities; window loss alone cannot authorize Home."""
    def __init__(self, app):
        self.app = valid_app(app)
        self.known = set()
        self.seen_window = False
        self.clear_since = None

    def sample(self, processes, windows, daemon_game, visible, menu_open, now):
        by_pid = {p['pid']: p for p in processes}
        observed = {p.get('app') for p in processes if p.get('app')}
        observed.update(w['app'] for w in windows if w['app'])
        if observed - {self.app}:
            raise RuntimeError('Another Steam App ID appeared; automatic return cancelled')
        # Literal Steam window identifiers are one identity source. Native
        # windows can instead use a generic app_id/class: associate those with
        # the exact-App-ID process identities already scoped by Driver.snapshot.
        target_windows = [w for w in windows if w['app'] == self.app]
        seeds = {p['pid'] for p in processes if p.get('app') == self.app}
        seeds.update(w['pid'] for w in target_windows)
        for pid in seeds:
            if pid in by_pid:
                self.known.add((pid, by_pid[pid]['start']))
        # Retain descendants even if they later reparent or lose SteamAppId.
        changed = True
        while changed:
            changed = False
            live = {pid: start for pid, start in self.known
                    if pid in by_pid and by_pid[pid]['start'] == start}
            for p in processes:
                key = (p['pid'], p['start'])
                parent_start = live.get(p['ppid'])
                if key not in self.known and parent_start is not None and p['start'] >= parent_start:
                    self.known.add(key)
                    changed = True
        target_windows = [w for w in windows if w['app'] == self.app or
                          (w['pid'] in by_pid and
                           (w['pid'], by_pid[w['pid']]['start']) in self.known)]
        if target_windows:
            self.seen_window = True
        # Zombies cannot execute, but still leave the process identity visible.
        alive = [p for p in processes if (p['pid'], p['start']) in self.known
                 and p['state'] not in ('Z', 'X')]
        clear = (self.seen_window and bool(self.known) and not alive and
                 not target_windows and not daemon_game and visible and not menu_open)
        self.clear_since = (now if self.clear_since is None else self.clear_since) if clear else None
        return {'seen_window': self.seen_window,
                'game_window_ids': [w['id'] for w in target_windows],
                'tracked': len(self.known),
                'alive': [p['pid'] for p in alive],
                'exit_evidence': self.clear_since is not None and now - self.clear_since >= 3.0}


def secure_directory(path):
    if path.is_symlink():
        raise RuntimeError(f'Unsafe runtime directory: {path}')
    path.mkdir(mode=0o700, exist_ok=True)
    stat = path.stat()
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise RuntimeError(f'Expected an owned private runtime directory: {path}')
    return path


def acquire_lock(path):
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        stat = os.fstat(fd)
        if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
            raise RuntimeError('Unsafe transaction lock')
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BaseException:
        os.close(fd)
        raise
    return fd


def native_command(root, app):
    return ['/usr/bin/setpriv', '--inh-caps=-all', '--ambient-caps=-all', '--',
            str(root / 'steamrtarm64/steam'), '-noverifyfiles', '-applaunch', valid_app(app)]


class Driver:
    def __init__(self):
        from gi.repository import Gio, GLib
        self.Gio, self.GLib = Gio, GLib
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        self.owner = self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (SERVICE,)),
            None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
        runtime = Path(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}'))
        sockets = ([Path(os.environ['SWAYSOCK'])] if os.environ.get('SWAYSOCK')
                   else list(runtime.glob('sway-ipc.*.sock')))
        if len(sockets) != 1:
            raise RuntimeError('Set SWAYSOCK to the active compositor socket; selection is ambiguous')
        self.socket = sockets[0]

    def call(self, method, signature=None, values=()):
        # Pin the unique bus owner: daemon restart cannot authorize a stale request.
        args = self.GLib.Variant(signature, values) if signature else None
        return self.bus.call_sync(self.owner, OBJECT, SERVICE, method, args, None,
            self.Gio.DBusCallFlags.NONE, 5000, None).unpack()

    def store(self):
        return self.call('StorefrontStatus')

    def action(self, action, generation):
        self.call('StorefrontAction', '(sss)', (action, 'steam', str(generation)))

    def unit(self):
        result = subprocess.run(['systemctl', '--user', '--no-pager', 'show', UNIT,
            '-p', 'ActiveState', '-p', 'MainPID', '-p', 'ControlGroup', '-p', 'InvocationID'],
            capture_output=True, text=True, timeout=5, check=True)
        props = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
        group = props.get('ControlGroup', '')
        if (props.get('ActiveState') != 'active' or not props.get('InvocationID') or
            not group.startswith('/') or '..' in group.split('/') or
            group.rsplit('/', 1)[-1] != UNIT):
            raise RuntimeError('Managed Steam service is not active with a valid invocation/cgroup')
        pid = int(props.get('MainPID', '0'))
        row = process_row(pid)
        if not in_group(process_group(pid), group):
            raise RuntimeError('Managed Steam MainPID is outside its service cgroup')
        return (props['InvocationID'], group, pid, row['start'])

    def tree(self):
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(2)
            stream.connect(str(self.socket))
            stream.sendall(b'i3-ipc' + struct.pack('<II', 0, 4))
            def read(size):
                out = b''
                while len(out) < size:
                    chunk = stream.recv(size - len(out))
                    if not chunk:
                        raise RuntimeError('Truncated Sway IPC response')
                    out += chunk
                return out
            header = read(14)
            size, kind = struct.unpack('<II', header[6:])
            if header[:6] != b'i3-ipc' or kind != 4 or size > 8 * 1024 * 1024:
                raise RuntimeError('Invalid Sway IPC response')
            return json.loads(read(size))

    def snapshot(self, group):
        processes = []
        inaccessible = 0
        directory = Path('/sys/fs/cgroup' + group)
        files = [directory / 'cgroup.procs'] + list(directory.glob('**/cgroup.procs'))
        pids = set()
        for path in set(files):
            try:
                pids.update(int(p) for p in path.read_text().split())
            except FileNotFoundError:
                continue  # A descendant scope can disappear between samples.
        for pid in sorted(pids):
            try:
                before = process_row(pid)
                if not in_group(process_group(pid), group):
                    continue
                try:
                    app = env_app(Path(f'/proc/{pid}/environ').read_bytes())
                except PermissionError:
                    inaccessible += 1
                    app = None
                after = process_row(pid)
                if before['start'] != after['start']:
                    raise RuntimeError('PID changed during sampling')
                after['app'] = app
                processes.append(after)
            except (FileNotFoundError, ProcessLookupError):
                continue
        windows = []
        by_pid = {p['pid'] for p in processes}
        for view in window_rows(self.tree()):
            if view['app'] and view['pid'] not in by_pid:
                raise RuntimeError('A Steam game window is outside the sampled managed scope; return cancelled')
            if view['pid'] in by_pid:
                windows.append(view)
        return processes, windows, inaccessible


def preflight(driver):
    if driver.call('Status')[0] not in ('Idle', 'Exited', 'Failed'):
        raise RuntimeError('An emulator session is active or changing state')
    if driver.call('InstalledGameStatus')[3] in BUSY:
        raise RuntimeError('An installed-game session is active or needs cleanup')
    if driver.call('MenuStatus')[0] or driver.call('MenuContext')[4]:
        raise RuntimeError('Close the Zaman menu and wait for its input release')
    transfer = driver.call('TransferContext')
    if transfer[1] not in ('idle', 'failed'):
        raise RuntimeError(f'Close Transfer before launch (phase={transfer[1]!r})')
    store = driver.store()
    if store[0] and (store[0] != 'steam' or store[2] != 'Running' or store[4]):
        raise RuntimeError('Return to Library and close existing gameplay first')
    # A Home-hidden managed service may have an empty StorefrontStatus; inspect
    # actual processes too, and reject any externally started Steam client.
    for path in Path('/proc').iterdir():
        if not path.name.isdigit():
            continue
        try:
            if path.stat().st_uid != os.getuid():
                continue
            row = process_row(int(path.name))
            if row['comm'].lower() not in ('steam', 'steamwebhelper'):
                continue
            group = process_group(row['pid'])
            if UNIT not in group.split('/'):
                raise RuntimeError('External Steam process detected; exit it normally before launch')
        except (FileNotFoundError, ProcessLookupError):
            pass
