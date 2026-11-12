#!/usr/bin/env python3
"""Export local Zaman source and deployment evidence without changing services.

Run as the Zaman desktop user on the SOURCE machine (Dragon Q6A).
The result is a source/deployment snapshot, not a CM2 installer.
"""
import argparse
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from datetime import datetime, timezone
from pathlib import Path

SKIP_DIRS = {'.git', '.ssh', '.gnupg', '.venv', 'venv', '__pycache__',
             'target', 'build', 'node_modules', '.cache', '.pytest_cache',
             'roms', 'bios', 'saves', 'savestates', 'backups', 'library-refresh'}
SKIP_SUFFIXES = {'.pyc', '.log', '.deb', '.AppImage', '.tar', '.gz', '.zip',
                 '.7z', '.rar', '.pem', '.key', '.p12', '.pfx'}
DEFAULT_UNITS = {'zaman-sessiond.service', 'zaman-quickshell.service',
                 'zaman-notifications.service', 'zaman-notification-events.service',
                 'zaman-menu-shortcut.service', 'zaman-transfer.service',
                 'zaman-transfer-gui.service'}
SECRET = re.compile(r'(password|passwd|secret|token|api.?key|credential)', re.I)
ABS_PATH = re.compile(r'/[^\s;{}\"\']+')


def run(argv):
    try:
        p = subprocess.run(argv, text=True, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, timeout=15)
        return {'argv': argv, 'returncode': p.returncode,
                'stdout': p.stdout, 'stderr': p.stderr}
    except (OSError, subprocess.TimeoutExpired) as e:
        return {'argv': argv, 'returncode': -1, 'stdout': '', 'stderr': str(e)}


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def redact(text):
    # Keep deployment structure, but remove obvious credential assignments.
    return '\n'.join('[REDACTED credential-bearing line]' if SECRET.search(line)
                     and ('=' in line or '--' in line) else line
                     for line in text.splitlines()) + '\n'


def source_hint(path):
    """Resolve a filesystem hint without retaining unreadable procfs magic links."""
    try:
        path = Path(path)
        if not path.is_absolute() or path.parts[1:2] in {('proc',), ('sys',), ('dev',)}:
            return None
        resolved = path.resolve(strict=True)
        if resolved.parts[1:2] in {('proc',), ('sys',), ('dev',)}:
            return None
        return resolved
    except (OSError, RuntimeError, ValueError):
        # Processes can exit and paths can become unreadable between discovery
        # and probing. These are optional hints, never a reason to elevate access.
        return None


def has_signature(path, signature):
    try:
        return path.is_dir() and all((path / item).is_file() for item in signature)
    except (OSError, RuntimeError, ValueError):
        return False


def proc_paths():
    paths = []
    for proc in Path('/proc').glob('[0-9]*'):
        try:
            if proc.stat().st_uid != os.getuid():
                continue
            cmd = (proc / 'cmdline').read_bytes().split(b'\0')
            if not any(b'zaman' in x or b'quickshell' in x for x in cmd):
                continue
            # readlink itself can be denied even for an otherwise visible PID.
            # Read the actual link first; non-strict Path.resolve can silently
            # leave an unresolved /proc/PID/exe path in its result.
            try:
                executable = source_hint((proc / 'exe').readlink())
                if executable is not None:
                    paths.append(executable)
            except (OSError, RuntimeError, ValueError):
                pass
            for arg in cmd:
                text = os.fsdecode(arg)
                hint = source_hint(text)
                if hint is not None:
                    paths.append(hint)
        except (OSError, ValueError):
            pass
    return paths


def ancestor_candidates(paths, signature):
    roots = []
    for path in paths:
        path = source_hint(path)
        if path is None:
            continue
        for parent in [path, *path.parents]:
            if has_signature(parent, signature):
                if parent not in roots:
                    roots.append(parent)
                break
    return roots


def choose_root(label, explicit, hints, fallbacks, signature, secondary_hints=()):
    if explicit:
        supplied = Path(explicit).expanduser().absolute()
        path = source_hint(supplied)
        if path is None or not has_signature(path, signature):
            raise ValueError(f'{label}: cannot read required files {signature} in {supplied}')
        return path
    choices = ancestor_candidates(hints, signature)
    if not choices:
        choices = ancestor_candidates(secondary_hints, signature)
    if not choices:
        choices = ancestor_candidates(fallbacks, signature)
    if len(choices) != 1:
        flag = {'sessiond': '--sessiond-dir', 'menu': '--menu-dir',
                'theme': '--theme-dir'}[label]
        found = ', '.join(map(str, choices)) or 'none'
        raise ValueError(f'{label}: candidates: {found}. Supply {flag} /actual/live/path')
    return choices[0]


class Snapshot:
    def __init__(self, archive, root, max_file_bytes):
        self.archive = archive
        self.root = root
        self.max_file_bytes = max_file_bytes
        self.entries = []
        self.skipped = []
        self.seen = set()
        self.errors = []

    def note(self, path, reason):
        self.skipped.append({'source': str(path), 'reason': reason})

    def add_bytes(self, name, data, mode=0o644, origin=None):
        if name in self.seen:
            return
        self.seen.add(name)
        info = tarfile.TarInfo(self.root + '/' + name)
        info.size = len(data)
        info.mode = mode
        info.mtime = int(datetime.now(timezone.utc).timestamp())
        self.archive.addfile(info, io.BytesIO(data))
        self.entries.append({'path': name, 'source': origin,
                             'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})

    def file(self, path, name):
        try:
            before = path.stat()
            if not stat.S_ISREG(before.st_mode):
                self.note(path, 'not a regular file')
                return
            if before.st_size > self.max_file_bytes:
                self.note(path, 'exceeds --max-file-mib; review before increasing limit')
                return
            data = path.read_bytes()
            after = path.stat()
            if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
                raise OSError('file changed during export; retry when edits finish')
            if data.startswith(b'\x7fELF'):
                self.note(path, 'compiled ELF: rebuild for target; source snapshot omits binary')
                return
            if b'-----BEGIN ' in data and b'PRIVATE KEY-----' in data:
                self.note(path, 'private key content')
                return
            mode = 0o755 if before.st_mode & 0o111 else 0o644
            self.add_bytes(name, data, mode, str(path))
        except OSError as e:
            self.errors.append({'source': str(path), 'error': str(e)})

    def tree(self, source, dest):
        source = source.resolve()
        if not source.exists():
            return
        if source.is_file():
            self.file(source, dest)
            return
        for current, dirs, files in os.walk(source, followlinks=False):
            current = Path(current)
            accepted = []
            for name in sorted(dirs):
                p = current / name
                if p.relative_to(source).parts in {
                    ('packaging', 'work'), ('packaging', 'out'), ('packaging', 'stage'),
                    ('packaging', 'runtime-src'), ('packaging', 'device-source')}:
                    continue
                if name in SKIP_DIRS or name.startswith(('build-', 'target-')):
                    continue
                if p.is_symlink():
                    self.note(p, 'directory symlink not followed; export target via --extra if needed')
                    continue
                accepted.append(name)
            dirs[:] = accepted
            for name in sorted(files):
                p = current / name
                if name == '.env' or name.startswith('.env.') or p.suffix in SKIP_SUFFIXES:
                    continue
                if name in {'.netrc', '.npmrc', '.pypirc', 'credentials.json'}:
                    continue
                if p.is_symlink():
                    try:
                        p.resolve().relative_to(source)
                    except (ValueError, OSError, RuntimeError):
                        self.note(p, 'external/broken symlink; export target via --extra if needed')
                        continue
                self.file(p, str(Path(dest) / p.relative_to(source)))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--sessiond-dir')
    ap.add_argument('--menu-dir')
    ap.add_argument('--theme-dir')
    ap.add_argument('--extra', action='append', default=[], metavar='PATH',
                    help='Additional Zaman source/helper path to capture')
    ap.add_argument('--output', type=Path)
    ap.add_argument('--max-file-mib', type=int, default=32)
    args = ap.parse_args()
    if os.getuid() == 0:
        ap.error('Run as your Zaman login user, without sudo, to inspect the correct user services.')
    if args.max_file_mib < 1:
        ap.error('--max-file-mib must be positive')
    home = Path.home()
    records = {}
    units = set(DEFAULT_UNITS)
    listed = run(['systemctl', '--user', 'list-unit-files', '--no-pager', '--no-legend'])
    for line in listed['stdout'].splitlines():
        parts = line.split()
        if parts and parts[0].startswith('zaman'):
            units.add(parts[0])
    service_hints = []
    for unit in sorted(units):
        command = run(['systemctl', '--user', 'show', unit, '-p', 'ExecStart',
                       '-p', 'WorkingDirectory', '-p', 'FragmentPath', '-p', 'DropInPaths',
                       '-p', 'ActiveState', '-p', 'UnitFileState'])
        records[unit] = command
        for value in ABS_PATH.findall(command['stdout']):
            hint = source_hint(value)
            if hint is not None:
                service_hints.append(hint)
    process_hints = proc_paths()
    # Live processes take precedence over fallback directories. Multiple live roots
    # are intentionally an error rather than silently picking an old checkout.
    hints = process_hints
    try:
        sessiond = choose_root('sessiond', args.sessiond_dir, hints,
            [home / 'projects/zaman-sprint4/zaman-sessiond', home / 'zaman-sessiond-publish'],
            ['Cargo.toml', 'src/main.rs'], secondary_hints=service_hints)
        menu = choose_root('menu', args.menu_dir, hints,
            [home / 'zaman-quickshell-prototype'], ['run.sh', 'qml/shell.qml'],
            secondary_hints=service_hints)
        theme = choose_root('theme', args.theme_dir, [],
            [home / '.config/pegasus-frontend/themes/zaman'], ['theme.qml', 'theme.cfg'])
    except ValueError as e:
        ap.error(str(e))
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    bundle = 'zaman-source-export-' + stamp
    output = (args.output or (home / (bundle + '.tar.gz'))).expanduser().absolute()
    if output.exists():
        ap.error(f'Output already exists: {output}; choose another --output')
    if not output.parent.is_dir():
        ap.error(f'Output directory does not exist: {output.parent}')
    for root in (sessiond, menu, theme):
        if output.is_relative_to(root):
            ap.error('Output must be outside all source directories')
    for label, root in [('sessiond', sessiond), ('resident-menu', menu), ('pegasus-theme', theme)]:
        print(f'{label}: {root}', flush=True)
    fd, temporary_name = tempfile.mkstemp(prefix='.zaman-export-', dir=output.parent)
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        with tarfile.open(temporary, 'w:gz', dereference=False) as archive:
            snap = Snapshot(archive, bundle, args.max_file_mib * 1024 * 1024)
            roots = {'sessiond': sessiond, 'resident-menu': menu, 'pegasus-theme': theme}
            git = {}
            for label, root in roots.items():
                snap.tree(root, 'sources/' + label)
                git[label] = {key: run(['git', '-C', str(root), *cmd])
                    for key, cmd in [('head', ['rev-parse', 'HEAD']),
                                     ('status', ['status', '--porcelain=v1', '--untracked-files=all'])]}
            # These are narrowly scoped Zaman directories, not whole user/system configs.
            deployment = [home / '.config/sway', Path('/etc/sway'),
                home / '.local/lib/zaman', home / '.local/libexec/zaman',
                Path('/usr/local/libexec/zaman'), Path('/usr/libexec/zaman'),
                Path('/usr/local/share/zaman'), Path('/usr/share/zaman'),
                Path('/etc/zaman'), Path('/etc/inputplumber'),
                home / '.config/zaman']
            deployment.extend(Path(x).expanduser().absolute() for x in args.extra)
            for p in deployment:
                if not p.exists():
                    continue
                # No /, /home, or whole-home export via --extra.
                if p.resolve() in {Path('/'), home.resolve(), Path('/home'), Path('/etc'), Path('/usr')}:
                    raise ValueError(f'Refusing broad deployment path: {p}')
                snap.tree(p, 'deployment/' + str(p.absolute()).lstrip('/'))
            # Preserve units as reference text, never auto-activate source-machine units.
            for unit, record in records.items():
                cat = run(['systemctl', '--user', 'cat', unit])
                if cat['returncode'] == 0:
                    snap.add_bytes('reference/user-units/' + unit + '.txt',
                                   redact(cat['stdout']).encode())
                # Capture script files referenced outside the main source roots.
                for raw in ABS_PATH.findall(record['stdout']):
                    p = source_hint(raw)
                    if p is not None:
                        try:
                            if p.is_file() and (p.suffix in {'.py', '.sh', '.qml'} or 'zaman' in p.name.lower()):
                                snap.tree(p, 'deployment/' + str(p.absolute()).lstrip('/'))
                        except (OSError, RuntimeError, ValueError) as error:
                            snap.note(p, 'unreadable optional service reference: ' + str(error))
            for directory in ['/usr/local/bin', str(home / '.local/bin'),
                '/usr/share/dbus-1/services', '/usr/local/share/dbus-1/services',
                '/usr/share/dbus-1/system.d', '/etc/dbus-1/system.d',
                '/usr/share/polkit-1/rules.d', '/etc/polkit-1/rules.d',
                '/etc/udev/rules.d', '/usr/lib/udev/rules.d',
                '/usr/share/applications', '/usr/local/share/applications',
                str(home / '.local/share/applications')]:
                p = Path(directory)
                if p.is_dir():
                    for f in sorted(p.iterdir()):
                        if 'zaman' in f.name.lower() and f.is_file():
                            snap.tree(f, 'deployment/' + str(f.absolute()).lstrip('/'))
            for unit in ['inputplumber.service', 'greetd.service', 'lightdm.service']:
                cat = run(['systemctl', 'cat', unit])
                if cat['returncode'] == 0:
                    snap.add_bytes('reference/system-units/' + unit + '.txt',
                                   redact(cat['stdout']).encode())
            binaries = []
            candidate_binaries = [sessiond / 'target/release' / n for n in
                ['zaman-sessiond', 'zamanctl', 'zaman-menu-dev-input', 'zaman-menu-dev-pair']]
            for command in ['qs', 'quickshell', 'pegasus-fe', 'sway', 'inputplumber']:
                located = shutil.which(command)
                if located:
                    candidate_binaries.append(Path(located))
            for p in candidate_binaries:
                if p.is_file():
                    with p.open('rb') as stream:
                        magic = stream.read(4)
                    binaries.append({'path': str(p), 'sha256': sha256_file(p),
                        'file': run(['file', str(p)]),
                        'symbol_versions': run(['readelf', '--version-info', str(p)])
                            if magic == b'\x7fELF' else None})
            inventory = {
                'uname': run(['uname', '-a']), 'libc': run(['getconf', 'GNU_LIBC_VERSION']),
                'packages': run(['dpkg-query', '-W', '-f=${binary:Package}\t${Version}\t${Architecture}\n']),
                'user_units': {k: {**v, 'stdout': redact(v['stdout']), 'stderr': redact(v['stderr'])}
                               for k, v in records.items()}, 'git': git, 'binaries': binaries}
            snap.add_bytes('reference/inventory.json', json.dumps(inventory, indent=2).encode())
            manifest = {'format': 1, 'created_utc': stamp, 'roots': {k: str(v) for k, v in roots.items()},
                'purpose': 'Local source and deployment evidence; NOT an installer.',
                'files': snap.entries.copy(), 'skipped': snap.skipped, 'errors': snap.errors,
                'excluded': ['Git object databases', 'Rust/CMake build directories', 'compiled ELF files',
                    'ROMs/BIOS/saves', 'SSH keys', 'NetworkManager connection profiles',
                    'notification history', 'whole home directories'],
                'review': 'Custom source/config may contain private data. Review before sharing. '
                          'Skipped symlinks, large files and read errors require review.'}
            snap.add_bytes('manifest.json', json.dumps(manifest, indent=2).encode())
        os.rename(temporary, output)
        digest = sha256_file(output)
        print(f'Archive: {output}')
        print(f'SHA256: {digest}')
        print(f'Captured: {len(snap.entries)} files; skipped: {len(snap.skipped)}; read errors: {len(snap.errors)}')
        if snap.errors:
            print('INCOMPLETE: inspect manifest.json read errors; resolve and re-export.', file=sys.stderr)
            return 1
        print('Review manifest.json, skipped entries and configuration before sharing. Services were not changed.')
        return 0
    except (OSError, ValueError, tarfile.TarError) as e:
        print(f'Export failed: {e}', file=sys.stderr)
        return 1
    finally:
        temporary.unlink(missing_ok=True)


if __name__ == '__main__':
    raise SystemExit(main())
