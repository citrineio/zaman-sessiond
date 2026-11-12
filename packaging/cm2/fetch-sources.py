#!/usr/bin/env python3
"""Resolve exact component commits or the live-device snapshot on this branch."""
import json
import re
import shutil
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
work = root / 'packaging/work'
work.mkdir(parents=True, exist_ok=True)
pins = json.loads((root / 'packaging/cm2/sources.json').read_text())
live = root / 'packaging/device-source/sources'
provenance = {}
for name, live_name in [('core', 'sessiond'), ('menu', 'resident-menu'), ('theme', 'pegasus-theme')]:
    dest = work / name
    if dest.exists():
        shutil.rmtree(dest)
    if live.is_dir():
        source = live / live_name
        if not source.is_dir():
            raise SystemExit(f'Incomplete live snapshot: {source}')
        shutil.copytree(source, dest, symlinks=False)
        provenance[name] = {'source': 'live-device-snapshot', 'directory': str(source.relative_to(root))}
    elif name == 'core':
        shutil.copytree(root, dest, ignore=shutil.ignore_patterns('.git', 'target', 'packaging', '.github'))
        provenance[name] = {'source': 'repository', 'commit': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()}
    else:
        pin = pins[name]
        subprocess.run(['git', 'init', '-q', str(dest)], check=True)
        subprocess.run(['git', '-C', str(dest), 'fetch', '--depth=1', 'https://github.com/' + pin['repository'] + '.git', pin['commit']], check=True)
        subprocess.run(['git', '-C', str(dest), 'checkout', '--detach', 'FETCH_HEAD'], check=True)
        provenance[name] = {'source': pin['repository'], 'commit': pin['commit']}
    required = {'core': ['Cargo.toml', 'Cargo.lock', 'src/main.rs'], 'menu': ['qml/shell.qml', 'run.sh'], 'theme': ['theme.qml', 'theme.cfg']}[name]
    for item in required:
        if not (dest / item).is_file():
            raise SystemExit(f'Missing {name} source: {item}')

# Live snapshots keep the daemon and separately deployed helper projects intact.
# Never overwrite them by reapplying an older menu archive during a device build.
if not live.is_dir():
    for src, target in [('zaman-menu-dev-input.rs', 'src/bin/zaman-menu-dev-input.rs'),
                        ('zaman-menu-dev-pair.rs', 'src/bin/zaman-menu-dev-pair.rs'),
                        ('display.rs', 'src/bin/zaman-menu-dev/display.rs'),
                        ('display_bridge.rs', 'src/bin/zaman-menu-dev/display_bridge.rs')]:
        shutil.copyfile(work / 'menu/src' / src, work / 'core' / target)

# Relocate only the known development installation prefixes. Source snapshots are
# preserved on the branch; this modifies the disposable build copy only.
replacements = {'/usr/local/bin/zamanctl': '/usr/bin/zamanctl',
                '/usr/local/libexec/zaman/': '/usr/libexec/zaman/',
                '/usr/local/share/zaman/': '/usr/share/zaman/'}
snapshot = root / 'packaging/device-source'
helper_builds = {}
if (snapshot / 'manifest.json').is_file():
    manifest = json.loads((snapshot / 'manifest.json').read_text())
    destinations = {'sessiond': '/usr/share/zaman/sessiond', 'resident-menu': '/usr/share/zaman/menu',
                    'pegasus-theme': '/usr/share/pegasus-frontend/themes/zaman'}
    for name, original in manifest.get('source_roots', {}).items():
        if name.startswith('helper-project-'):
            destinations[name] = '/usr/share/zaman/sessiond'
        if name in destinations:
            replacements[original] = destinations[name]
    helper_builds = manifest.get('helper_builds', {})
    for identity in set(helper_builds.values()):
        if not re.fullmatch(r'[a-f0-9]{12}', identity):
            raise SystemExit('Invalid helper project identity')
        source = live / 'helper-projects' / identity
        if not (source / 'Cargo.toml').is_file() or not (source / 'Cargo.lock').is_file():
            raise SystemExit('Incomplete live helper source: ' + identity)
        target = work / 'helper-projects' / identity
        if target.exists():
            shutil.rmtree(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, target)
        provenance['helper-project-' + identity] = {'source': 'live-device-snapshot', 'directory': str(source.relative_to(root))}
    for item in manifest.get('files', []):
        if item['path'].startswith('deployment/'):
            original = '/' + item['path'][len('deployment/'):]
            destination = '/usr/libexec/zaman/device/' + original.lstrip('/')
            if original.endswith('.desktop'):
                destination = '/usr/share/applications/' + Path(original).name
            replacements[original] = destination
            directory = str(Path(original).parent)
            if '/libexec/zaman' in directory or '/lib/zaman/' in directory:
                replacements.setdefault(directory + '/', str(Path(destination).parent) + '/')
replacements = dict(sorted(replacements.items(), key=lambda pair: len(pair[0]), reverse=True))
relocation_pattern = re.compile('|'.join(re.escape(key) for key in replacements))
(work / 'relocation.json').write_text(json.dumps(replacements, indent=2) + '\n')
(work / 'helper-builds.json').write_text(json.dumps(helper_builds, indent=2) + '\n')
for name in ['core', 'menu', 'theme', 'helper-projects']:
    for path in (work / name).rglob('*'):
        if path.is_file() and '.git' not in path.parts and path.suffix in {'.rs', '.py', '.qml', '.js', '.sh', '.toml'}:
            content = path.read_text()
            content = relocation_pattern.sub(lambda match: replacements[match.group()], content)
            path.write_text(content)
(work / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
print(json.dumps(provenance, indent=2))
