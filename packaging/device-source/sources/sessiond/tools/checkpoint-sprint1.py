#!/usr/bin/env python3
"""Checkpoint the accepted device sources. No build, install or service changes."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid


def run(args, cwd=None):
    return subprocess.run(args, cwd=cwd, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE).stdout


def git(repo, *args):
    return run(['git', '-C', str(repo), *args])


def digest(data):
    return hashlib.sha256(data).hexdigest()


def safe_file(repo, relative):
    path = repo / relative
    if path.is_symlink() or repo.resolve() not in path.resolve().parents:
        raise RuntimeError(f'Unsafe path: {path}')
    return path


def preflight(repos, manifest):
    for name, repo in repos.items():
        if git(repo, 'branch', '--show-current').decode().strip() != 'sprint1/storefronts':
            raise RuntimeError(f'{repo}: expected sprint1/storefronts')
        if git(repo, 'diff', '--name-only', '--diff-filter=U'):
            raise RuntimeError(f'{repo}: unresolved merge conflict')
        for key in ('user.name', 'user.email'):
            if not git(repo, 'config', key).strip():
                raise RuntimeError(f'{repo}: configure git {key}')
        allowed = set(manifest['paths'][name])
        staged = set(git(repo, 'diff', '--cached', '--name-only', '-z').decode().split('\0')) - {''}
        if staged - allowed:
            raise RuntimeError(f'{repo}: unrelated staged paths: {sorted(staged - allowed)}')
        for relative in allowed:
            path = safe_file(repo, relative)
            created = relative in manifest['documents'].get(name, {}) or relative == 'tools/sprint1-checkpoint.json'
            if not path.is_file() and not created:
                raise RuntimeError(f'Missing checkpoint source: {path}')
        for relative, expected in manifest['required'].get(name, {}).items():
            path = safe_file(repo, relative)
            if not path.is_file() or digest(path.read_bytes()) != expected:
                raise RuntimeError(f'{path}: does not match the tested recovery update')
        for relative, item in manifest['documents'].get(name, {}).items():
            path = safe_file(repo, relative)
            actual = digest(path.read_bytes()) if path.exists() else None
            if actual not in (item['before'], digest(item['content'].encode())):
                raise RuntimeError(f'{path}: unexpected document edit; preserved')
    shortcut = (repos['zaman-menu'] / 'shortcut/zaman-menu-shortcut.py').read_text()
    if "status not in ('library', 'game', 'storefront')" not in shortcut:
        raise RuntimeError('The tested Start+Select storefront guard is missing')
    compile(shortcut, 'zaman-menu-shortcut.py', 'exec')


def device_preflight(home, repos):
    import tomllib
    config = home / '.config/zaman/storefronts.d/steam.toml'
    with config.open('rb') as stream:
        steam = tomllib.load(stream)
    if steam.get('argv') != [str(home / '.local/bin/zaman-steam')]:
        raise RuntimeError('Steam argv does not use the accepted wrapper directly; preserve and inspect it')
    daemon = repos['zaman-sessiond'] / 'target/release/zaman-sessiond'
    pid = run(['systemctl', '--user', 'show', 'zaman-sessiond.service',
               '-p', 'MainPID', '--value']).decode().strip()
    if not pid.isdigit() or int(pid) == 0:
        raise RuntimeError('The accepted daemon is not running')
    if digest(daemon.read_bytes()) != digest(Path(f'/proc/{pid}/exe').read_bytes()):
        raise RuntimeError('The running daemon does not match the candidate binary')
    return {'daemon_pid': int(pid), 'daemon_sha256': digest(daemon.read_bytes()),
            'steam_argv': steam['argv'], 'native_guide': steam.get('native_guide')}


def refresh_checksums(repo):
    path = repo / 'SHA256.json'
    values = json.loads(path.read_text())
    for relative in sorted(values):
        candidate = safe_file(repo, relative)
        if not candidate.is_file():
            raise RuntimeError(f'Missing checksum input: {candidate}')
        values[relative] = digest(candidate.read_bytes())
    path.write_text(json.dumps(values, indent=2, sort_keys=True) + '\n')
    (repo / 'SHA256SUMS').write_text(''.join(f'{values[key]}  {key}\n' for key in sorted(values)))


def checkpoint(repos, manifest, backup, record, script):
    # Preserve every source version and index diff before the first mutation.
    backup.mkdir(parents=True, mode=0o700)
    for name, repo in repos.items():
        saved = backup / name
        saved.mkdir(mode=0o700)
        for label, args in [('working.patch', ('diff', '--binary', 'HEAD')),
                            ('index.patch', ('diff', '--binary', '--cached'))]:
            (saved / label).write_bytes(git(repo, *args))
        git(repo, 'bundle', 'create', str(saved / 'before.bundle'), 'HEAD')
        (saved / 'status.txt').write_bytes(git(repo, 'status', '--short'))
        for relative in manifest['paths'][name]:
            source = safe_file(repo, relative)
            if source.is_file():
                target = saved / 'files' / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)
    (backup / 'record.json').write_text(json.dumps(record, indent=2) + '\n')
    # Documents and checksum metadata only; never replace runtime source/config.
    for name, documents in manifest['documents'].items():
        for relative, item in documents.items():
            target = safe_file(repos[name], relative)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(item['content'])
    target = repos['zaman-sessiond'] / 'tools/checkpoint-sprint1.py'
    if target.resolve() != script.resolve():
        shutil.copy2(script, target)
    shutil.copy2(script.with_name('sprint1-checkpoint.json'),
                 repos['zaman-sessiond'] / 'tools/sprint1-checkpoint.json') if \
        script.with_name('sprint1-checkpoint.json').resolve() != \
        (repos['zaman-sessiond'] / 'tools/sprint1-checkpoint.json').resolve() else None
    refresh_checksums(repos['zaman-menu'])
    # Recheck tested runtime sources immediately before staging.
    preflight(repos, manifest)
    for name, repo in repos.items():
        git(repo, 'add', '--', *manifest['paths'][name])


def commit_and_tag(repos, tag):
    refs = {}
    for name, repo in repos.items():
        changed = subprocess.run(['git', '-C', str(repo), 'diff', '--cached', '--quiet'])
        if changed.returncode == 1:
            git(repo, 'commit', '-m', f'{name}: checkpoint accepted Sprint 1 recovery')
        elif changed.returncode != 0:
            raise RuntimeError(f'{repo}: could not inspect index')
        git(repo, 'tag', '-a', tag, '-m',
            'Q6A owner-verified Sprint 1 recovery; Windows game compatibility deferred')
        refs[name] = git(repo, 'rev-parse', 'HEAD').decode().strip()
    return refs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='commit and tag the accepted device sources')
    parser.add_argument('--push', action='store_true', help='push branch and checkpoint tag after local checkpoint')
    args = parser.parse_args()
    if args.push and not args.apply:
        parser.error('--push requires --apply')
    os.umask(0o077)
    home = Path.home()
    repos = {'zaman-sessiond': home / 'zaman-sessiond-publish-sprint1',
             'zaman-menu': home / 'zaman-quickshell-prototype-sprint1',
             'dukkan': home / 'projects/dukkan-sprint1'}
    script = Path(__file__).resolve()
    manifest = json.loads(script.with_name('sprint1-checkpoint.json').read_text())
    preflight(repos, manifest)
    target_manifest = repos['zaman-sessiond'] / 'tools/sprint1-checkpoint.json'
    if target_manifest.exists() and target_manifest.read_bytes() != script.with_name('sprint1-checkpoint.json').read_bytes():
        raise RuntimeError('Existing checkpoint manifest differs; preserved')
    record = device_preflight(home, repos)
    if args.push:
        for name, repo in repos.items():
            remote = git(repo, 'remote', 'get-url', '--push', 'origin').decode().strip()
            if remote not in (f'git@github.com:citrineio/{name}.git',
                              f'https://github.com/citrineio/{name}.git'):
                raise RuntimeError(f'{repo}: unexpected push origin {remote}')
    print('Preflight passed. Tested source hashes, running daemon and shortcut verified.')
    if not args.apply:
        print('No files changed. Run with --apply to checkpoint; add --push to publish.')
        return
    tag = 'snapshot/sprint1-accepted-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6]
    backup = home / '.local/state/zaman/sprint1-checkpoints' / tag.split('/')[-1]
    print(f'Backup: {backup}', flush=True)
    record['tag'] = tag
    checkpoint(repos, manifest, backup, record, script)
    # Private device configuration backup; never staged or pushed.
    for relative in ('.config/zaman/storefronts.d/steam.toml',
                     '.config/zaman/storefronts.d/dukkan.toml', '.local/bin/zaman-steam',
                     'zaman-lab/sway-ab/config', 'zaman-lab/sway-ab/outputs.conf'):
        source = home / relative
        if source.is_file() and not source.is_symlink():
            target = backup / 'device' / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
    for unit in ('zaman-sessiond.service', 'zaman-quickshell.service', 'zaman-menu-shortcut.service'):
        source = home / '.config/systemd/user' / (unit + '.d') / '90-sprint1.conf'
        if source.is_file() and not source.is_symlink():
            target = backup / 'device/services' / (unit + '.conf')
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
    record['commits'] = commit_and_tag(repos, tag)
    (backup / 'record.json').write_text(json.dumps(record, indent=2) + '\n')
    for name, repo in repos.items():
        git(repo, 'bundle', 'create', str(backup / name / 'accepted.bundle'), 'HEAD', 'refs/tags/' + tag)
        print(f'{name}: {record["commits"][name]}')
    print(f'Checkpoint tag: {tag}')
    print('No rebuild, install or service restart performed.')
    if args.push:
        for name, repo in repos.items():
            try:
                git(repo, 'push', '--atomic', 'origin',
                    'HEAD:refs/heads/sprint1/storefronts', 'refs/tags/' + tag)
                print(f'Published {name}', flush=True)
            except subprocess.CalledProcessError as error:
                print(error.stderr.decode(), file=sys.stderr)
                raise RuntimeError(f'{name}: push failed. Local checkpoint and bundles are safe at {backup}; '
                                   'earlier repositories may already be published. No force push was attempted.')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
