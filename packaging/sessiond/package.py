#!/usr/bin/env python3
"""Package the built Zaman daemon for native Debian 13 ARM64."""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BINARIES = ('zaman-sessiond', 'zamanctl', 'zaman-menu-dev-input', 'zaman-menu-dev-pair')


def capture(*cmd, **kwargs):
    return subprocess.check_output(cmd, text=True, **kwargs).strip()


def copy(src, stage, target):
    if not src.is_file():
        raise FileNotFoundError(src)
    dst = stage / target
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)
    return dst


def copy_dir(src, stage, target):
    if not src.is_dir():
        raise FileNotFoundError(src)
    shutil.copytree(src, stage / target, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns('.git', '__pycache__', '*.pyc'))


def write(stage, target, content):
    dst = stage / target
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(content)
    return dst


def shlibdeps(binaries):
    with tempfile.TemporaryDirectory() as wd:
        control = Path(wd) / 'debian/control'
        control.parent.mkdir(parents=True)
        control.write_text('Source: zaman-sessiond\n\nPackage: zaman-sessiond\nArchitecture: any\nDescription: Zaman daemon\n')
        lines = capture('dpkg-shlibdeps', '-O', *(f'-e{p}' for p in binaries), cwd=wd)
        for line in lines.splitlines():
            if line.startswith('shlibs:Depends='):
                return line.partition('=')[2]
        raise RuntimeError('No dependency metadata returned by dpkg-shlibdeps')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--version', default=None)
    parser.add_argument('--output', type=Path, default=ROOT / 'packaging/out/sessiond')
    args = parser.parse_args()
    with (ROOT / 'Cargo.toml').open('rb') as f:
        version = args.version or tomllib.load(f)['package']['version'] + '+git.0'
    if not re.fullmatch(r'[0-9][A-Za-z0-9.+~:-]*', version):
        parser.error('Invalid Debian version')
    architecture = capture('dpkg', '--print-architecture')
    if architecture != 'arm64':
        parser.error('Build this package natively on Debian 13 ARM64')
    binaries = [ROOT / 'target/release' / name for name in BINARIES]
    for src in binaries:
        if not src.is_file():
            raise FileNotFoundError(f'Missing compiled binary: {src}')
    dependencies = shlibdeps(binaries)
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='zaman-deb-') as tmp:
        stage = Path(tmp)
        copy(binaries[0], stage, 'usr/libexec/zaman-sessiond')
        copy(binaries[1], stage, 'usr/bin/zamanctl')
        for src in binaries[2:]:
            copy(src, stage, f'usr/libexec/zaman/{src.name}')
        compat = stage / 'usr/libexec/zaman/zaman-sessiond'
        compat.parent.mkdir(parents=True, exist_ok=True)
        compat.symlink_to('../zaman-sessiond')
        copy_dir(ROOT / 'assets', stage, 'usr/share/zaman/sessiond/assets')
        copy_dir(ROOT / 'registry', stage, 'usr/share/zaman')
        copy_dir(ROOT / 'interfaces', stage, 'usr/share/zaman/interfaces')
        copy(ROOT / 'catalog/games.json', stage, 'usr/share/zaman/catalog.json')
        for item in (ROOT / 'tools').iterdir():
            if item.is_file() and ((item.suffix == '.py' and not item.name.startswith('test-')) or item.name in {'zaman-toasts', 'zaman-steam'}):
                copy(item, stage, f'usr/libexec/zaman/{item.name}')
        copy_dir(ROOT / 'tools/transfer/receiver', stage, 'usr/libexec/zaman/transfer')
        unit = (ROOT / 'dist/systemd/user/zaman-sessiond.service').read_text()
        if '[Service]\n' not in unit:
            raise ValueError('Unexpected systemd service file')
        unit = unit.replace('[Service]\n', '[Service]\nEnvironment=ZAMAN_PEGASUS_LIBRARY_SYNC_HELPER=/usr/libexec/zaman/pegasus-library-sync.py\nEnvironment=ZAMAN_INSTALLED_GAME_HELPER=/usr/libexec/zaman/flatpak-game-session.py\n', 1)
        write(stage, 'usr/lib/systemd/user/zaman-sessiond.service', unit)
        copy(ROOT / 'dist/dbus-1/services/com.kawnelectro.Zaman.Session1.service', stage, 'usr/share/dbus-1/services/com.kawnelectro.Zaman.Session1.service')
        copy(ROOT / 'README.md', stage, 'usr/share/doc/zaman-sessiond/README.md')
        copy(ROOT / 'CHANGELOG.md', stage, 'usr/share/doc/zaman-sessiond/CHANGELOG.md')
        installed_size = sum(p.stat().st_size for p in stage.rglob('*') if p.is_file()) // 1024
        control = (f'Package: zaman-sessiond\nVersion: {version}\nArchitecture: {architecture}\n'
                   'Maintainer: Kawn Electro <packages@kawnelectro.com>\n'
                   'Section: games\nPriority: optional\n'
                   f'Installed-Size: {installed_size}\nDepends: {dependencies}, dbus-user-session, python3\n'
                   'Recommends: python3-gi, gir1.2-glib-2.0, flatpak, xdg-utils\n'
                   'Homepage: https://github.com/citrineio/zaman-sessiond\n'
                   'Description: Zaman game session supervisor and CLI\n'
                   ' Per-user D-Bus daemon, control CLI, controller helpers and registry.\n'
                   ' Installing this package does not enable or restart the session daemon.\n')
        write(stage, 'DEBIAN/control', control)
        output = args.output / f'zaman-sessiond_{version}_{architecture}.deb'
        subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(stage), str(output)], check=True)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    (args.output / 'SHA256SUMS').write_text(f'{digest}  {output.name}\n')
    print(f'Built {output} SHA256={digest}')


if __name__ == '__main__':
    main()
