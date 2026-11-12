#!/usr/bin/env python3
"""Publish the current device's Zaman source directly to a GitHub build branch."""
import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

REPO = 'citrineio/zaman-sessiond'
RAW = 'https://raw.githubusercontent.com/' + REPO + '/main/packaging/cm2/'


def command(argv, **kwargs):
    return subprocess.run(argv, check=True, **kwargs)


def git(checkout, *args):
    return command(['git', '-c', 'credential.helper=', '-c', 'credential.helper=!gh auth git-credential', '-C', str(checkout), *args])


def publish():
    ap = argparse.ArgumentParser(description=__doc__)
    for name in ['sessiond-dir', 'menu-dir', 'theme-dir']:
        ap.add_argument('--' + name)
    ap.add_argument('--source-host', help='Export via SSH from the Q6A; publish using this computer\'s authenticated gh')
    ap.add_argument('--extra', action='append', default=[])
    ap.add_argument('--no-wait', action='store_true')
    args = ap.parse_args()
    if os.getuid() == 0:
        ap.error('Run on the source Q6A as the Zaman user, without sudo.')
    command(['gh', 'auth', 'status'])
    login = subprocess.check_output(['gh', 'api', 'user', '--jq', '.login'], text=True).strip()
    branch = 'device-build/' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:6]
    with tempfile.TemporaryDirectory(prefix='zaman-publish-') as td:
        temporary = Path(td)
        exporter = Path(__file__).resolve().with_name('export-zaman-stack.py')
        if not exporter.is_file():
            exporter = temporary / 'export-zaman-stack.py'
            with urllib.request.urlopen(RAW + 'export-zaman-stack.py', timeout=60) as response:
                exporter.write_bytes(response.read())
        archive = temporary / 'device.tar.gz'
        argv = [sys.executable, str(exporter), '--output', str(archive)]
        for name in ['sessiond-dir', 'menu-dir', 'theme-dir']:
            value = getattr(args, name.replace('-', '_'))
            if value:
                argv.extend(['--' + name, value])
        for value in args.extra:
            argv.extend(['--extra', value])
        if args.source_host:
            import re
            if not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.@-]*', args.source_host):
                ap.error('Invalid SSH host')
            remote_dir = subprocess.check_output(['ssh', args.source_host, 'mktemp -d /tmp/zaman-publish.XXXXXXXX'], text=True).strip()
            if not remote_dir.startswith('/tmp/zaman-publish.') or '/' in remote_dir[len('/tmp/'):]:
                raise RuntimeError('Unexpected remote temporary directory')
            try:
                remote_argv = ['python3', '-', '--output', remote_dir + '/snapshot.tar.gz', *argv[4:]]
                # argv[4:] starts after the local archive argument.
                command(['ssh', args.source_host, shlex.join(remote_argv)], input=exporter.read_bytes())
                command(['scp', args.source_host + ':' + remote_dir + '/snapshot.tar.gz', str(archive)])
            finally:
                cleanup = ['python3', '-c', 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); (p/"snapshot.tar.gz").unlink(missing_ok=True); p.rmdir()', remote_dir]
                subprocess.run(['ssh', args.source_host, shlex.join(cleanup)], check=False)
        else:
            command(argv)
        checkout = temporary / 'repo'
        command(['git', '-c', 'credential.helper=', '-c', 'credential.helper=!gh auth git-credential',
                 'clone', '--depth=1', 'https://github.com/' + REPO + '.git', str(checkout)])
        git(checkout, 'checkout', '-b', branch)
        stage = checkout / 'packaging/device-source'
        stage.mkdir(parents=True)
        with tarfile.open(archive) as tf:
            members = tf.getmembers()
            prefix = members[0].name.split('/')[0] + '/'
            manifest = json.load(tf.extractfile(prefix + 'manifest.json'))
            if manifest.get('errors'):
                raise RuntimeError('Incomplete export; no source has been pushed.')
            expected = {x['path']: x['sha256'] for x in manifest['files']}
            import re
            inventory = json.load(tf.extractfile(prefix + 'reference/inventory.json'))
            active_helpers = set()
            for record in inventory.get('user_units', {}).values():
                for path in re.findall(r'/[^\s;{}\"\']+', record.get('stdout', '')):
                    name = 'deployment/' + path.lstrip('/')
                    if name in expected and Path(path).suffix in {'.py', '.sh'}:
                        active_helpers.add(name)
            public_files = []
            for member in members:
                if not member.isfile() or not member.name.startswith(prefix):
                    continue
                relative = PurePosixPath(member.name[len(prefix):])
                if relative.is_absolute() or '..' in relative.parts:
                    raise RuntimeError('Unsafe archive path')
                # Do not publish machine inventories, units, settings, input maps,
                # catalogs, or state. Sources and installed helper code are sufficient.
                selected = relative.parts[0] == 'sources'
                if relative.parts[0] == 'deployment':
                    original = '/' + '/'.join(relative.parts[1:])
                    selected = ('/libexec/' in original or '/lib/zaman/' in original or '/bin/zaman' in original) and relative.suffix in {'.py', '.sh', '.qml', '.js'}
                    selected = selected or (relative.suffix == '.desktop' and 'zaman' in relative.name.lower())
                if not selected:
                    continue
                data = tf.extractfile(member).read()
                if hashlib.sha256(data).hexdigest() != expected[str(relative)]:
                    raise RuntimeError('Snapshot checksum mismatch: ' + str(relative))
                target = stage.joinpath(*relative.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)
                public_files.append({'path': str(relative), 'sha256': expected[str(relative)]})
            public_manifest = {'format': 1, 'created_utc': manifest['created_utc'],
                               'source': 'current device sources and installed helper code',
                               'source_roots': manifest['roots'],
                               'active_helpers': sorted(active_helpers),
                               'files': public_files,
                               'source_omissions': [x for x in manifest['skipped'] if any(str(x.get('source','')).startswith(root + '/') for root in manifest['roots'].values())]}
            (stage / 'manifest.json').write_text(json.dumps(public_manifest, indent=2) + '\n')
        git(checkout, 'add', '--', 'packaging/device-source')
        git(checkout, '-c', 'user.name=' + login, '-c', 'user.email=' + login + '@users.noreply.github.com',
            'commit', '-m', 'Build CM2 packages from current device source ' + branch.rsplit('/', 1)[1])
        git(checkout, 'push', 'origin', 'HEAD:refs/heads/' + branch)
    print('Source published: https://github.com/' + REPO + '/tree/' + branch, flush=True)
    print('Builds: https://github.com/' + REPO + '/actions/workflows/cm2-packages.yml', flush=True)
    if args.no_wait:
        return
    run_id = None
    for _ in range(30):
        data = json.loads(subprocess.check_output(['gh', 'run', 'list', '--repo', REPO, '--branch', branch,
            '--workflow', 'cm2-packages.yml', '--json', 'databaseId', '--limit', '1'], text=True))
        if data:
            run_id = str(data[0]['databaseId'])
            break
        time.sleep(2)
    if run_id is None:
        raise RuntimeError('Source was pushed but no workflow run was found; inspect the Actions URL above.')
    command(['gh', 'run', 'watch', run_id, '--repo', REPO, '--exit-status'])
    print('Packages published: https://github.com/' + REPO + '/releases', flush=True)


if __name__ == '__main__':
    try:
        publish()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error))
