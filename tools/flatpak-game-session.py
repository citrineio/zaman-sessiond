#!/usr/bin/env python3
"""Owned Flatpak instance lifecycle foundation; standalone Q6A probe, not Play API."""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time
import tempfile
import uuid

UNIT = 'zaman-dukkan-game-probe.service'
WORKSPACE = 'dukkan-game-probe'
GRACE_SECONDS = 8
APPS = {
    'org.supertuxproject.SuperTux': 'SuperTux',
    'net.supertuxkart.SuperTuxKart': 'SuperTuxKart',
    'org.wesnoth.Wesnoth': 'The Battle for Wesnoth',
    'org.openttd.OpenTTD': 'OpenTTD',
    'org.opensurge2d.OpenSurge': 'Surge Engine',
}
SESSION_KEYS = ('WAYLAND_DISPLAY', 'DISPLAY', 'SWAYSOCK', 'XAUTHORITY',
                'LANG', 'LC_ALL', 'XDG_SESSION_TYPE')


def call(argv, timeout=5):
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or f'{argv[0]} exited {result.returncode}')
    return result.stdout


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream)
            stream.write('\n')
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def load_json(path):
    if path.is_symlink():
        raise RuntimeError('Refusing a symlink state file')
    return json.loads(path.read_text())


def runtime_root(managed=False):
    root = Path(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')) / (
        'zaman-installed-game' if managed else 'zaman-flatpak-probe')
    if root.is_symlink():
        raise RuntimeError('Unsafe probe runtime directory')
    root.mkdir(mode=0o700, exist_ok=True)
    stat = root.stat()
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise RuntimeError('Probe runtime directory must be owned by this user with mode 0700')
    return root


def cgroup(pid):
    for row in Path(f'/proc/{int(pid)}/cgroup').read_text().splitlines():
        if row.startswith('0::'):
            return row[3:]
    raise RuntimeError('A unified cgroup is required')


def process_start(pid):
    # comm may contain spaces and parentheses; fields after its final ')' start at field 3.
    text = Path(f'/proc/{int(pid)}/stat').read_text()
    return text.rsplit(')', 1)[1].split()[19]


def instances(output):
    result = []
    for row in output.splitlines():
        fields = row.split()
        if len(fields) == 3 and fields[0].isdigit() and fields[1].isdigit():
            result.append({'instance': fields[0], 'pid': int(fields[1]), 'app': fields[2]})
    return result


def owned_row(rows, app, instance, expected_pid=None):
    if not isinstance(instance, str) or not instance.isdigit():
        raise RuntimeError('Invalid owned instance ID')
    matches = [row for row in rows if row['instance'] == instance]
    if len(matches) > 1:
        raise RuntimeError('Ambiguous Flatpak instance')
    if not matches:
        return None
    row = matches[0]
    if row['app'] != app or (expected_pid is not None and row['pid'] != expected_pid):
        raise RuntimeError(f'Flatpak instance identity changed; no application was stopped: instance={instance}, expected app={app} pid={expected_pid}, observed app={row["app"]} pid={row["pid"]}')
    return row


def wait_identity(driver, app, instance, child, timeout=8, clock=time.monotonic, pause=time.sleep):
    deadline = clock() + timeout
    expected_scope = f'app-flatpak-{app}-{instance}.scope'
    while clock() < deadline and child.poll() is None:
        row = owned_row(driver.rows(), app, instance)
        if row and row['pid'] > 0:
            try:
                before = process_start(row['pid'])
                group = cgroup(row['pid'])
                after = process_start(row['pid'])
                current = owned_row(driver.rows(), app, instance)
                if (current and current['pid'] == row['pid'] and before == after
                    and group.rsplit('/', 1)[-1] == expected_scope):
                    return {'instance_pid': row['pid'], 'instance_start': before, 'cgroup': group}
            except OSError:
                pass  # the wrapper or its scope may still be getting established
        pause(0.1)
    raise RuntimeError('Owned instance PID/scope did not become ready before the startup deadline')


def window_rows(tree, group, group_reader=cgroup):
    result = []
    def walk(node, workspace=''):
        if node.get('type') == 'workspace':
            workspace = node.get('name', '')
        if node.get('pid'):
            try:
                actual = group_reader(node['pid'])
            except (OSError, RuntimeError):
                actual = ''
            if actual == group or actual.startswith(group + '/'):
                row = {key: node.get(key) for key in ('id', 'pid', 'name', 'app_id', 'focused', 'fullscreen_mode')}
                row['workspace'] = workspace
                result.append(row)
        for child in node.get('nodes', []) + node.get('floating_nodes', []):
            walk(child, workspace)
    walk(tree)
    return result


class Driver:
    def rows(self):
        return instances(call(['/usr/bin/flatpak', 'ps', '--columns=instance,pid,application:f']))

    def check_install(self, app):
        if app not in APPS:
            raise RuntimeError('Application is not in the curated probe catalog')
        ref = call(['/usr/bin/flatpak', 'info', '--user', '--arch=aarch64', '--show-ref', app, 'stable']).strip()
        if ref != f'app/{app}/aarch64/stable':
            raise RuntimeError('Expected an installed user aarch64/stable application')
        if any(row['app'] == app for row in self.rows()):
            raise RuntimeError('Quit the existing application normally before starting the managed probe')

    def owned(self, record):
        # Zero is Flatpak's startup placeholder, never a committed process identity.
        expected_pid = record.get('instance_pid')
        if expected_pid == 0:
            expected_pid = None
        row = owned_row(self.rows(), record['app'], record['instance'], expected_pid)
        if row is None:
            return None
        if row['pid'] <= 0:
            raise RuntimeError('Owned instance PID is not ready; nothing was stopped')
        if 'instance_start' in record and record['instance_start'] != process_start(row['pid']):
            raise RuntimeError('Instance PID was reused; no application was stopped')
        if expected_pid is None:
            # Recover only an incomplete record from this launcher pipe. Require
            # its exact app/instance scope before issuing an instance-targeted stop.
            group = cgroup(row['pid'])
            if group.rsplit('/', 1)[-1] != f'app-flatpak-{record["app"]}-{record["instance"]}.scope':
                raise RuntimeError('Incomplete record has an unexpected live scope; nothing was stopped')
        return row

    def tree(self):
        return json.loads(call(['/usr/bin/swaymsg', '-t', 'get_tree']))

    def sway(self, command):
        replies = json.loads(call(['/usr/bin/swaymsg', '-r', command]))
        if not replies or any(not item.get('success') for item in replies):
            raise RuntimeError('Sway rejected the owned-window command: ' + json.dumps(replies))

    def place(self, record, previous):
        if not self.owned(record):
            return previous, []
        views = window_rows(self.tree(), record['cgroup'])
        if not views:
            return previous, []
        primary = next((row for row in views if row['id'] == previous), views[0])
        for row in views:
            if row['workspace'] != WORKSPACE:
                self.sway(f'[con_id={int(row["id"])}] move container to workspace "{WORKSPACE}"')
        if previous != primary['id']:
            self.sway(f'[con_id={int(primary["id"])}] floating disable, border none, fullscreen enable, focus')
        return primary['id'], window_rows(self.tree(), record['cgroup'])

    def close_windows(self, record):
        if self.owned(record):
            for row in window_rows(self.tree(), record['cgroup']):
                self.sway(f'[con_id={int(row["id"])}] kill')

    def force(self, record):
        if self.owned(record):
            # Numeric instance ID only: never an app ID or all matching applications.
            call(['/usr/bin/flatpak', 'kill', record['instance']])

    def library(self):
        self.sway('workspace --no-auto-back-and-forth "library"')


class ManagedDriver(Driver):
    """Native sessiond owns placement and focus; this driver owns only the sandbox."""
    def check_install(self, app):
        super().check_install(app)
        if any(row['app'] in APPS for row in self.rows()):
            raise RuntimeError('Another curated Flatpak game is already running')

    def place(self, record, previous):
        if not self.owned(record):
            return previous, []
        return previous, window_rows(self.tree(), record['cgroup'])

    def library(self):
        pass


def read_instance(fd, child, timeout=12):
    deadline = time.monotonic() + timeout
    data = bytearray()
    while time.monotonic() < deadline:
        if select.select([fd], [], [], min(0.2, max(0, deadline-time.monotonic())))[0]:
            chunk = os.read(fd, 128)
            if not chunk:
                break
            data.extend(chunk)
            if len(data) > 64:
                raise RuntimeError('Oversized Flatpak instance identity')
            # The ID fits in one pipe write. Accept a complete numeric payload
            # without waiting for the launching process to close an inherited fd.
            if bytes(data).strip(b' \r\n\0').isdigit():
                break
        if child.poll() is not None:
            # Drain a final write after a very short-lived launch.
            if not select.select([fd], [], [], 0)[0]:
                break
    value = data.decode('ascii').strip(' \r\n\0')
    if not value.isdigit():
        raise RuntimeError('Flatpak did not supply a numeric instance ID; inspect the probe journal')
    return value


def run_game(app, nonce, driver=None, managed=False):
    driver = driver or (ManagedDriver() if managed else Driver())
    root = runtime_root(managed)
    record = {'app': app, 'name': APPS.get(app, ''), 'nonce': nonce,
              'phase': 'Starting', 'launcher_pid': os.getpid(),
              'launcher_start': process_start(os.getpid()), 'instance': '',
              'error': '', 'forced': False, 'windows': []}
    stopped = [False]
    def on_signal(signum, frame):
        stopped[0] = True
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, on_signal)
    child = None
    failure = None
    lock_fd = None
    try:
        if managed:
            lock_fd = os.open(root / 'transaction.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeError('A Flatpak transaction or managed game is already active')
        driver.check_install(app)
        atomic_json(root / 'session.json', record)
        read_fd, write_fd = os.pipe()
        try:
            child = subprocess.Popen(['/usr/bin/flatpak', 'run', '--user', '--arch=aarch64',
                '--branch=stable', '--die-with-parent', f'--instance-id-fd={write_fd}', app],
                pass_fds=(write_fd,))
            os.close(write_fd)
            write_fd = None
            record['instance'] = read_instance(read_fd, child)
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)
        record.update(wait_identity(driver, app, record['instance'], child))
        record['phase'] = 'Running'
        atomic_json(root / 'session.json', record)
        primary = None
        stop_deadline = None
        close_sent = False
        force_attempts = 0
        while True:
            request = root / 'close.json'
            if request.exists() and load_json(request).get('nonce') == nonce:
                stopped[0] = True
            row = driver.owned(record)
            if row is None:
                break
            if child.poll() is not None:
                # A launcher exit must not leave its known sandbox running.
                stopped[0] = True
                if stop_deadline is None:
                    stop_deadline = 0
            if stopped[0]:
                record['phase'] = 'Stopping'
                if stop_deadline is None:
                    stop_deadline = time.monotonic() + GRACE_SECONDS
                if not close_sent:
                    try:
                        driver.close_windows(record)
                    except (RuntimeError, subprocess.SubprocessError) as error:
                        record['error'] = str(error)
                    close_sent = True
                if time.monotonic() >= stop_deadline:
                    if force_attempts:
                        raise RuntimeError('Owned Flatpak instance remained alive after forced stop')
                    driver.force(record)
                    record['forced'] = True
                    force_attempts += 1
                    stop_deadline = time.monotonic() + 5
            else:
                primary, views = driver.place(record, primary)
                record['windows'] = views
            atomic_json(root / 'session.json', record)
            time.sleep(0.2)
        if child.poll() is None:
            child.wait(timeout=5)
        record['exit_code'] = child.returncode
        if not stopped[0] and child.returncode != 0:
            failure = f'Game launcher exited with code {child.returncode}'
    except Exception as error:
        failure = str(error)
    finally:
        # Cleanup is targeted to the instance obtained through this launch's pipe.
        if record.get('instance'):
            try:
                driver.force(record)
                deadline = time.monotonic() + 5
                while driver.owned(record) and time.monotonic() < deadline:
                    time.sleep(0.1)
                if driver.owned(record):
                    raise RuntimeError('Owned Flatpak instance is still running')
            except Exception as error:
                record['phase'] = 'CleanupFailed'
                record['error'] = str(error)
                atomic_json(root / 'session.json', record)
                print(json.dumps(record), flush=True)
                # Retain ownership state; a new probe must refuse until explicit cleanup succeeds.
                failure = failure or str(error)
        if child is not None and child.poll() is None:
            child.kill()
            child.wait(timeout=5)
        if record['phase'] != 'CleanupFailed':
            record['phase'] = 'Failed' if failure else 'Exited'
            record['error'] = failure or ''
            atomic_json(root / 'session.json', record)
            try:
                driver.library()
            except Exception as error:
                record['focus_error'] = str(error)
                atomic_json(root / 'session.json', record)
        print(json.dumps(record), flush=True)
        if lock_fd is not None:
            os.close(lock_fd)
    return 1 if failure else 0


def import_session():
    environment = call(['/usr/bin/systemctl', '--user', 'show-environment'])
    for row in environment.splitlines():
        key, sep, value = row.partition('=')
        if sep and key in SESSION_KEYS:
            os.environ[key] = value
    os.environ['XDG_RUNTIME_DIR'] = f'/run/user/{os.getuid()}'
    os.environ['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=' + os.environ['XDG_RUNTIME_DIR'] + '/bus'
    if not os.environ.get('SWAYSOCK') or not os.environ.get('WAYLAND_DISPLAY'):
        raise RuntimeError('Start the graphical Sway session first')


def active_state(unit):
    return call(['/usr/bin/systemctl', '--user', 'show', unit, '-p', 'ActiveState', '--value']).strip()


def launch(app):
    import_session()
    for unit in (UNIT, 'zaman-game.service', 'zaman-store-steam.service', 'zaman-store-dukkan.service'):
        if active_state(unit) not in ('', 'inactive', 'failed'):
            raise RuntimeError(f'Quit {unit} normally before this isolated probe')
    driver = Driver()
    driver.check_install(app)
    root = runtime_root()
    state = root / 'session.json'
    if state.exists():
        previous = load_json(state)
        if previous.get('instance') and driver.owned(previous):
            raise RuntimeError('The previous probe instance is still alive; run close first')
    nonce = uuid.uuid4().hex
    Driver().sway(f'workspace --no-auto-back-and-forth "{WORKSPACE}"')
    argv = ['/usr/bin/systemd-run', '--user', '--quiet', '--collect', '--unit=' + UNIT,
            '--property=Type=exec', '--property=KillMode=control-group', '--property=TimeoutStopSec=15s']
    for key in SESSION_KEYS + ('XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS'):
        if key in os.environ:
            argv.append('--setenv=' + key + '=' + os.environ[key])
    argv += ['/usr/bin/python3', str(Path(__file__).resolve()), 'run', app, nonce]
    try:
        call(argv)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if state.exists():
                record = load_json(state)
                if record.get('nonce') == nonce:
                    if record['phase'] == 'Running' and record['windows']:
                        print(json.dumps(record, indent=2))
                        return
                    if record['phase'] in ('Exited', 'Failed', 'CleanupFailed'):
                        raise RuntimeError(record.get('error') or 'Game exited during startup')
            time.sleep(0.1)
        raise RuntimeError('Probe readiness timed out. Run status and inspect the journal before retrying')
    except Exception:
        atomic_json(root / 'close.json', {'nonce': nonce})
        Driver().library()
        raise


def close():
    import_session()
    root = runtime_root()
    record = load_json(root / 'session.json')
    nonce = record['nonce']
    if active_state(UNIT) not in ('', 'inactive', 'failed'):
        atomic_json(root / 'close.json', {'nonce': nonce})
    else:
        # Recovery of a recorded instance after a supervisor failure.
        Driver().force(record) if record.get('instance') else None
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        current = load_json(root / 'session.json')
        if current['nonce'] != nonce:
            raise RuntimeError('Probe session changed; no further close request was sent')
        if active_state(UNIT) in ('', 'inactive', 'failed'):
            if record.get('instance') and Driver().owned(record):
                raise RuntimeError('The owned instance is still alive; inspect the journal')
            Driver().library()
            if current['phase'] not in ('Exited', 'Failed'):
                current['previous_error'] = current.get('error', '')
                current['error'] = ''
                current['phase'] = 'Exited'
                current['recovery_cleanup'] = True
                atomic_json(root / 'session.json', current)
            print(json.dumps(current, indent=2))
            return
        time.sleep(0.2)
    raise RuntimeError('Probe is still closing. Inspect status/journal before retrying')


def recover_managed():
    """Reconcile exact recorded instance after daemon or supervisor loss."""
    root = runtime_root(True)
    state = root / 'session.json'
    if not state.exists():
        return
    record = load_json(state)
    if record.get('app') not in APPS or not record.get('nonce'):
        raise RuntimeError('Invalid managed game record; refusing broad cleanup')
    driver = ManagedDriver()
    unit_state = active_state('zaman-installed-game.service')
    if unit_state not in ('', 'inactive', 'failed'):
        atomic_json(root / 'close.json', {'nonce': record['nonce']})
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and active_state('zaman-installed-game.service') not in ('', 'inactive', 'failed'):
            time.sleep(0.2)
        if active_state('zaman-installed-game.service') not in ('', 'inactive', 'failed'):
            raise RuntimeError('Managed game supervisor did not stop')
    if record.get('instance'):
        driver.force(record)
        deadline = time.monotonic() + 5
        while driver.owned(record) and time.monotonic() < deadline:
            time.sleep(0.1)
        if driver.owned(record):
            raise RuntimeError('Owned managed Flatpak instance survived recovery')
    record['phase'] = 'Exited'
    record['error'] = ''
    record['recovery_cleanup'] = True
    atomic_json(state, record)


def crash_probe():
    """Qualification only: kill this exact supervisor and observe parent-death cleanup."""
    import_session()
    record = load_json(runtime_root() / 'session.json')
    pid = int(call(['/usr/bin/systemctl', '--user', 'show', UNIT, '-p', 'MainPID', '--value']).strip())
    if pid <= 1 or pid != record['launcher_pid'] or process_start(pid) != record['launcher_start']:
        raise RuntimeError('Probe supervisor identity changed; nothing was killed')
    os.kill(pid, signal.SIGKILL)
    deadline = time.monotonic() + 5
    alive = True
    while time.monotonic() < deadline:
        alive = bool(record.get('instance') and Driver().owned(record))
        if not alive:
            break
        time.sleep(0.1)
    record['instance_alive'] = alive
    record['unit_state'] = active_state(UNIT)
    record['parent_death_check'] = 'failed' if alive else 'passed'
    print(json.dumps(record, indent=2))
    if alive:
        raise RuntimeError('Sandbox survived supervisor death. Run close to target the recorded instance; paste this output')
    Driver().library()


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('launch', 'status', 'close', 'crash', 'run', 'run-managed', 'recover-managed'))
    parser.add_argument('app', nargs='?', default='org.supertuxproject.SuperTux')
    parser.add_argument('nonce', nargs='?')
    args = parser.parse_args()
    if args.action in ('run', 'run-managed'):
        if args.app not in APPS or not args.nonce or len(args.nonce) != 32 or any(c not in '0123456789abcdef' for c in args.nonce):
            raise RuntimeError('Invalid owned launch request')
        return run_game(args.app, args.nonce, managed=args.action == 'run-managed')
    if args.action == 'recover-managed':
        recover_managed()
        return 0
    if args.action == 'launch':
        launch(args.app)
    elif args.action == 'close':
        close()
    elif args.action == 'crash':
        crash_probe()
    else:
        import_session()
        record = load_json(runtime_root() / 'session.json')
        record['unit_state'] = active_state(UNIT)
        record['instance_alive'] = bool(record.get('instance') and Driver().owned(record))
        print(json.dumps(record, indent=2))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print('ERROR: ' + str(error), file=sys.stderr)
        sys.exit(1)
