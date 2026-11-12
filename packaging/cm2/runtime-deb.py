#!/usr/bin/env python3
"""Package private Qt/Quickshell and infer system dependencies from ELF links."""
import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path
from importlib.machinery import SourceFileLoader

here = Path(__file__).resolve().parent
pkg = SourceFileLoader('build_debs', str(here / 'build-debs.py')).load_module()
ap = argparse.ArgumentParser()
ap.add_argument('--version', required=True)
args = ap.parse_args()
pins = json.loads((here / 'sources.json').read_text())
prefix = Path('/opt/zaman/runtime')
root = pkg.new_package('zaman-quickshell-runtime')
dest = root / 'opt/zaman/runtime'
shutil.copytree(prefix, dest, symlinks=True)
for name in ['include', 'mkspecs', 'doc', 'examples', 'lib/cmake', 'lib/pkgconfig']:
    path = dest / name
    if path.exists():
        shutil.rmtree(path)
for path in list(dest.rglob('*.a')) + list(dest.glob('.zaman-*')):
    path.unlink()
# Retain runtime binaries, libraries, plugins, QML and license notices. Qt's SDK
# tools are not required by the installed menu; qs is recreated as a local link.
for path in (dest / 'bin').iterdir():
    if path.name not in {'quickshell', 'qs'}:
        if path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()
pkg.write(root, 'usr/libexec/zaman/qs', '''#!/bin/sh
set -eu
exec /opt/zaman/runtime/bin/quickshell "$@"
''', 0o755)
pkg.write(root, 'opt/zaman/runtime/bin/qt.conf', '''[Paths]
Prefix=..
Libraries=lib
Plugins=plugins
QmlImports=qml
''')

owners = {}
for path in dest.rglob('*'):
    if not path.is_file() or path.is_symlink():
        continue
    with path.open('rb') as stream:
        if stream.read(4) != b'\x7fELF':
            continue
    linked = subprocess.run(['ldd', str(path)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if 'not found' in linked.stdout:
        raise SystemExit(f'Unresolved runtime libraries for {path}:\n{linked.stdout}')
    for line in linked.stdout.splitlines():
        match = re.search(r'(?:=>\s+)?(/[^\s]+)', line)
        if not match:
            continue
        library = Path(match.group(1))
        if str(library).startswith('/opt/zaman/runtime/'):
            continue
        found = None
        for candidate in [library, library.resolve()]:
            query = subprocess.run(['dpkg-query', '-S', str(candidate)], text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            if query.returncode == 0:
                found = query.stdout.splitlines()[0].split(': ')[0]
                break
        if found is None:
            raise SystemExit(f'No Debian owner for linked library {library}')
        package = found.split(':')[0]
        version = subprocess.check_output(['dpkg-query', '-W', '-f=${Version}', found], text=True).strip()
        owners[package] = version
version = pins['quickshell'].lstrip('v') + '+qt' + pins['qt'] + '-' + args.version
pkg.WORK.mkdir(parents=True, exist_ok=True)
provenance = {'qt': {'version': pins['qt'], 'source': 'https://github.com/qt'},
              'quickshell': {'version': pins['quickshell'], 'source': 'https://github.com/quickshell-mirror/quickshell'}}
(pkg.WORK / 'provenance.json').write_text(json.dumps(provenance))
arch = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
pkg.build_package(root, 'zaman-quickshell-runtime', version, arch,
                  ', '.join(f'{name} (>= {v})' for name, v in sorted(owners.items())),
                  'Private Qt and Quickshell runtime for Zaman on Debian 12')
