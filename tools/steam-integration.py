#!/usr/bin/env python3
"""Automatic local Steam collection and Big Picture -> App ID launcher.

The observer is a small transient user service, not a replacement sessiond.
It uses only existing storefront operations; no Rust build is needed.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import tomllib
import uuid

import zaman_steam_library as library
import zaman_steam_session as session

OBSERVER = 'zaman-steam-launch.service'
HERE = Path(__file__).resolve()


def runtime():
    base = Path(os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid())))
    return session.secure_directory(base / 'zaman-steam-launch')


def event(key, title, body):
    # Steam notice switches affect notices, not actionable launch failures.
    import socket
    try:
        base = Path(os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid())))
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(0.2)
            sock.connect(str(base / 'zaman-notifications/control.sock'))
            sock.sendall((json.dumps({'action': 'event', 'source': 'Zaman', 'key': key,
                'title': title, 'body': body[:800], 'category': 'error'}) + '\n').encode())
            sock.recv(256)
    except OSError:
        pass


def write_status(phase, **fields):
    data = dict(phase=phase, updated=time.time(), **fields)
    library.atomic(runtime() / 'status.json', json.dumps(data) + '\n')
    print(json.dumps(data), flush=True)


def read_status():
    try:
        return json.loads((runtime() / 'status.json').read_text())
    except FileNotFoundError:
        return {'phase': 'Idle'}


def native_binary(root):
    path = root / 'steamrtarm64/steam'
    with path.open('rb') as stream:
        header = stream.read(20)
    if (len(header) != 20 or header[:6] != b'\x7fELF\x02\x01' or
            int.from_bytes(header[18:20], 'little') != 183 or not os.access(path, os.X_OK)):
        raise RuntimeError('Expected the native ARM64 Steam client in the discovered root')
    return path


def check_game(app, root=None):
    if root is None and not os.environ.get('ZAMAN_STEAM_ROOT'):
        # Reuse the root discovered by the exporter. A custom root configured
        # for its service need not be inherited by the Pegasus process.
        root = library.previous_inventory(library.locations()['state'] / 'inventory.json').get('root')
    root = library.steam_root(root)
    inventory = library.scan(root)
    game = inventory['games'].get(app)
    if not game or not game.get('available'):
        raise RuntimeError('The selected App ID is not a ready installed game in Steam metadata')
    native_binary(root)
    profile = library.locations()['config'] / 'zaman/storefronts.d/steam.toml'
    config = tomllib.loads(profile.read_text())
    if config.get('native_guide') is not True or not config.get('argv'):
        raise RuntimeError('The accepted native-Guide Steam profile is required')
    return root, game


def live_identities(identities):
    live = []
    for pid, birth in identities:
        try:
            row = session.process_row(pid)
            if row['start'] == birth and row['state'] not in ('Z', 'X'):
                live.append(pid)
        except (FileNotFoundError, ProcessLookupError):
            pass
    return live


def client_stopped():
    result = subprocess.run(['systemctl', '--user', '--no-pager', 'show', session.UNIT,
        '-p', 'ActiveState', '-p', 'MainPID'], capture_output=True, text=True, timeout=5)
    props = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    if props.get('ActiveState') not in ('inactive', 'failed') or props.get('MainPID', '0') != '0':
        return False
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid == os.getuid() and (process / 'comm').read_text().strip().lower() in ('steam', 'steamwebhelper'):
                return False
        except (FileNotFoundError, ProcessLookupError):
            pass
    return True


def recover_record(record):
    if record.get('phase') in ('Idle', 'Completed', 'Failed'):
        return True
    identities = record.get('tracked_identities', [])
    if record.get('sender_identity'):
        identities = identities + [record['sender_identity']]
    if not client_stopped() or live_identities(identities):
        return False
    if record.get('dispatch_pending'):
        # A crash in the Popen/identity publication gap is not safe to recover
        # from process absence alone; require a new login/runtime directory.
        return False
    write_status('Failed', app=record.get('app'), error='Previous launch ended; Steam and retained processes are closed')
    return True


def play(app):
    app = library.app_id(app)
    lockdir = session.secure_directory(runtime().parent / 'zaman-installed-game')
    with os.fdopen(session.acquire_lock(lockdir / 'transaction.lock'), 'r+'):
        if not recover_record(read_status()):
            raise RuntimeError('A previous Steam launch still needs recovery; inspect status and close Steam normally')
        driver = session.Driver()
        session.preflight(driver)
        root, game = check_game(app)
        token = uuid.uuid4().hex
        evidence = session.Evidence(app)
        sender = None
        record = {'app': app, 'name': game['name'], 'token': token,
                  'tracked_identities': [], 'sender_identity': None, 'dispatch_pending': False}
        started = time.monotonic()
        def stage(phase, **extra):
            record.update(extra)
            write_status(phase, **record, elapsed=round(time.monotonic() - started, 3))
        try:
            stage('WaitingClient')
            # Existing managed Big Picture wrapper emits the startup toast.
            # When the client is in the background, this resumes it without exec.
            driver.action('launch', 0)
            deadline = time.monotonic() + 180
            while True:
                store = driver.store()
                if store[0] == 'steam' and store[2] == 'Running':
                    identity = driver.unit()
                    processes, windows, unreadable = driver.snapshot(identity[1])
                    if store[4] or any(p.get('app') for p in processes) or any(w['app'] for w in windows):
                        raise RuntimeError('Steam already has gameplay; no request sent')
                    if store[3] == 'steam' and session.client_visible(windows):
                        generation = store[1]; break
                if time.monotonic() >= deadline:
                    raise RuntimeError('Steam did not become ready; inspect its login/update screen')
                time.sleep(0.5)
            session.preflight(driver)
            # Revalidate the selected installation immediately before dispatch.
            check_game(app, root)
            if driver.unit() != identity or driver.store()[1] != generation:
                raise RuntimeError('Steam changed before launch request')
            env = os.environ.copy()
            env.update(LD_LIBRARY_PATH=f'{root}/steamrtarm64:{root}/steamrtarm64/video')
            log = runtime() / (token + '.sender.log')
            stage('Dispatching', dispatch_pending=True)
            with log.open('xb') as output:
                sender = subprocess.Popen(session.native_command(root, app), env=env,
                    stdout=output, stderr=subprocess.STDOUT)
            try:
                row = session.process_row(sender.pid)
                sender_identity = [sender.pid, row['start']]
            except (FileNotFoundError, ProcessLookupError):
                if sender.poll() is None:
                    raise RuntimeError('Sender identity unavailable')
                sender_identity = None
            stage('Requested', dispatch_pending=False, sender_identity=sender_identity,
                  sender_log=str(log), invocation=identity[0], generation=generation)
            requested = time.monotonic()
            was_running = False
            while True:
                store = driver.store()
                if store[:3] != ('steam', generation, 'Running') or driver.unit() != identity:
                    raise RuntimeError('Steam context changed; automatic return cancelled')
                processes, windows, unreadable = driver.snapshot(identity[1])
                sampled = {(p['pid'], p['start']) for p in processes}
                if live_identities(evidence.known - sampled):
                    raise RuntimeError('A tracked game process left the Steam scope; automatic return cancelled')
                state = evidence.sample(processes, windows, store[4], session.client_visible(windows),
                    driver.call('MenuStatus')[0], time.monotonic())
                if sorted(evidence.known) != record['tracked_identities']:
                    record['tracked_identities'] = sorted(evidence.known)
                    stage('Running' if state['seen_window'] else 'Requested')
                if state['seen_window'] and not was_running:
                    was_running = True
                    stage('Running', first_window_seconds=round(time.monotonic() - requested, 3))
                if sender.poll() not in (None, 0):
                    raise RuntimeError('Steam rejected the launch request; inspect sender log')
                if sender.poll() is None and time.monotonic() - requested > 20:
                    raise RuntimeError('Steam launch sender has not completed')
                if not state['seen_window'] and time.monotonic() - requested > 180:
                    raise RuntimeError('No verified game window appeared; inspect Steam')
                if state['exit_evidence'] and sender.poll() == 0:
                    if driver.unit() != identity or driver.store() != store or driver.call('MenuStatus')[0]:
                        raise RuntimeError('Context changed before return to Pegasus')
                    driver.action('home', generation)
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        if driver.unit() != identity:
                            raise RuntimeError('Steam changed after Home')
                        if driver.call('ForegroundStatus')[0] == 'library' and driver.store()[0] == '':
                            stage('Completed', steam_still_running=True); return
                        time.sleep(0.25)
                    raise RuntimeError('Pegasus foreground restoration was not confirmed')
                time.sleep(0.5)
        except Exception as error:
            # Keep the lock and observer alive through uncertain startup/gameplay.
            # Nothing kills a client or game and no compensating Home is issued.
            record['tracked_identities'] = sorted(evidence.known)
            stage('RecoveryRequired', error=str(error))
            event('steam-launch-error:' + token, 'Steam launch needs attention',
                  str(error) + '. Exit the game and close Steam normally before retrying.')
            while True:
                if recover_record(read_status()):
                    return
                time.sleep(2)


def launch(app):
    app = library.app_id(app)
    check_game(app)
    # Unique unit rejects repeated selections even before the lock is acquired.
    command = ['systemd-run', '--user', '--collect', '--wait', '--unit=' + OBSERVER,
               '--property=Type=exec', '--property=KillMode=control-group',
               '--property=TimeoutStopSec=5', '--property=Restart=no']
    for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'SWAYSOCK', 'XDG_RUNTIME_DIR',
                 'DBUS_SESSION_BUS_ADDRESS', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME',
                 'XDG_STATE_HOME', 'ZAMAN_STEAM_ROOT'):
        if name in os.environ:
            command += ['--setenv=' + name + '=' + os.environ[name]]
    result = subprocess.run(command + ['--', sys.executable, str(HERE), '_play', app], check=False)
    if result.returncode:
        raise RuntimeError('Launch adapter did not complete; inspect steam-integration.py status before retrying')


def watch():
    from gi.repository import Gio, GLib
    monitors = {}
    pending = None
    first_event = 0
    loop = GLib.MainLoop()
    def run():
        nonlocal pending
        pending = None
        try:
            observer = subprocess.run(['systemctl', '--user', 'is-active', OBSERVER],
                                      capture_output=True, text=True, timeout=5)
            if observer.stdout.strip() in ('active', 'activating', 'deactivating'):
                # Defer provider reload while Pegasus's launcher is active.
                # The periodic reconciliation catches up after game exit.
                return False
            result = library.sync(launcher=HERE)
            directories = {library.steam_root(), library.steam_root() / 'appcache',
                           library.steam_root() / 'appcache/librarycache'}
            for value in result['libraries']:
                path = Path(value)
                directories.update((path, path / 'steamapps'))
            for directory in directories:
                if directory not in monitors and directory.is_dir():
                    monitor = Gio.File.new_for_path(str(directory)).monitor_directory(Gio.FileMonitorFlags.NONE, None)
                    monitor.connect('changed', changed)
                    monitors[directory] = monitor
            for directory in list(monitors):
                if directory not in directories or not directory.is_dir():
                    monitors.pop(directory).cancel()
            print(json.dumps({'games': len(result['games']), 'changed': result['changed'],
                              'unclassified': result['unclassified'], 'diagnostics': result['diagnostics']}), flush=True)
        except (OSError, ValueError, RuntimeError) as error:
            print('Steam sync retained last good output: ' + str(error), file=sys.stderr, flush=True)
        return False
    def changed(*unused):
        nonlocal pending, first_event
        now = time.monotonic()
        if pending is None:
            first_event = now
        elif now - first_event < 3:
            GLib.source_remove(pending)
        else:
            return
        pending = GLib.timeout_add(1000, run)
    def reconcile():
        changed(); return True
    GLib.timeout_add_seconds(30, reconcile)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGTERM, lambda: (loop.quit(), False)[1])
    run(); loop.run()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('sync', 'watch', 'launch', '_play', 'status', 'recover'))
    parser.add_argument('app', nargs='?', type=library.app_id)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    try:
        if args.action == 'sync':
            print(json.dumps(library.sync(dry_run=args.dry_run, launcher=HERE), indent=2))
        elif args.action == 'watch':
            watch()
        elif args.action in ('launch', '_play'):
            if args.app is None:
                parser.error('launch requires a discovered App ID')
            (launch if args.action == 'launch' else play)(args.app)
        elif args.action == 'status':
            print(json.dumps(read_status(), indent=2))
        elif not recover_record(read_status()):
            raise RuntimeError('Close Steam and retained processes normally; unresolved dispatch requires logout/login')
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        if args.action in ('launch', '_play'):
            event('steam-launch-error:' + uuid.uuid4().hex, 'Steam game could not start', str(error))
        print('Steam integration: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
