#!/usr/bin/env python3
"""Build fresh-install packages from the resolved, target-built source checkpoint."""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / 'packaging/work'
OUT = ROOT / 'packaging/out'
STAGE = ROOT / 'packaging/stage'
MAINTAINER = 'Kawn Electro <packages@kawnelectro.com>'


def write(root, name, content, mode=0o644):
    dest = root / name.lstrip('/')
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(content.strip() + '\n')
    dest.chmod(mode)
    return dest


def copy(source, root, name):
    dest = root / name.lstrip('/')
    dest.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        shutil.copytree(source, dest, dirs_exist_ok=True, ignore=shutil.ignore_patterns('.git', '__pycache__', '*.pyc'))
    else:
        shutil.copy2(source, dest)


def link(root, name, target):
    dest = root / name.lstrip('/')
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.symlink_to(target)


def new_package(name):
    root = STAGE / name
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    return root


def build_package(root, name, version, arch, depends, description, recommends=''):
    size = sum(p.stat().st_size for p in root.rglob('*') if p.is_file() and not p.is_symlink()) // 1024
    control = f'''Package: {name}
Version: {version}
Architecture: {arch}
Maintainer: {MAINTAINER}
Section: games
Priority: optional
Installed-Size: {size}
Depends: {depends}
Homepage: https://github.com/citrineio/zaman-sessiond
Description: {description}
 Zaman component package built for Debian 12. Graphical session activation is explicit.
'''
    if recommends:
        control += 'Recommends: ' + recommends + '\n'
    write(root, 'DEBIAN/control', control)
    write(root, f'usr/share/doc/{name}/build-source.json',
          json.dumps(json.loads((WORK / 'provenance.json').read_text()), indent=2))
    OUT.mkdir(exist_ok=True, parents=True)
    path = OUT / f'{name}_{version}_{arch}.deb'
    subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(root), str(path)], check=True)
    return path


def shlibdeps(binaries):
    # Run in a minimal Debian source context; dependencies come from the actual
    # linked target binaries and installed Debian library metadata.
    with tempfile.TemporaryDirectory() as td:
        debian = Path(td) / 'debian'
        debian.mkdir()
        (debian / 'control').write_text('Source: zaman-sessiond\n\nPackage: zaman-sessiond\nArchitecture: any\nDescription: Zaman\n')
        output = subprocess.check_output(['dpkg-shlibdeps', '-O', *['-e' + str(p) for p in binaries]], cwd=td, text=True)
    return next(line.partition('=')[2] for line in output.splitlines() if line.startswith('shlibs:Depends='))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--version', required=True)
    args = ap.parse_args()
    if not re.fullmatch(r'[0-9][A-Za-z0-9.+~:-]*', args.version):
        ap.error('invalid Debian version')
    v = args.version
    arch = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
    core, menu, theme = [WORK / x for x in ['core', 'menu', 'theme']]
    bins = [core / 'target/release' / n for n in ['zaman-sessiond', 'zamanctl', 'zaman-menu-dev-input', 'zaman-menu-dev-pair']]
    for p in bins:
        if not p.is_file():
            raise SystemExit(f'Build missing: {p}')
    deps = shlibdeps(bins)
    packages = []
    root = new_package('zaman-sessiond')
    for binary in bins:
        target = 'usr/bin/zamanctl' if binary.name == 'zamanctl' else 'usr/libexec/zaman/' + binary.name
        copy(binary, root, target)
        (root / target).chmod(0o755)
    copy(core / 'assets', root, 'usr/share/zaman/sessiond/assets')
    for n in ['zaman-menu-dev-input', 'zaman-menu-dev-pair']:
        link(root, 'usr/share/zaman/sessiond/target/release/' + n, '/usr/libexec/zaman/' + n)
    link(root, 'usr/share/zaman/sessiond/tools', '/usr/libexec/zaman')
    for kind in ['systems.d', 'emulators.d']:
        if (core / 'registry' / kind).is_dir():
            copy(core / 'registry' / kind, root, 'usr/share/zaman/' + kind)
    if (core / 'catalog/games.json').is_file():
        copy(core / 'catalog/games.json', root, 'usr/share/zaman/catalog.json')
    copy(core / 'interfaces', root, 'usr/share/zaman/interfaces')
    write(root, 'usr/share/dbus-1/services/com.kawnelectro.Zaman.Session1.service', '''
[D-BUS Service]
Name=com.kawnelectro.Zaman.Session1
Exec=/usr/libexec/zaman/zaman-sessiond
SystemdService=zaman-sessiond.service
''')
    policy = core / 'dist/libexec/zaman-inputplumber-policy'
    if policy.is_file():
        copy(policy, root, 'usr/libexec/zaman-inputplumber-policy')
        (root / 'usr/libexec/zaman-inputplumber-policy').chmod(0o755)
        copy(core / 'dist/systemd/system/inputplumber.service.d/50-zaman-management.conf', root,
             'usr/lib/systemd/system/inputplumber.service.d/50-zaman-management.conf')
    packages.append(build_package(root, 'zaman-sessiond', v, arch, deps + ', dbus-user-session, python3',
                                  'Zaman session supervision, CLI and input helpers', 'inputplumber'))

    root = new_package('zaman-menu')
    for name in ['qml', 'assets', 'notifications', 'notification-shell.qml', 'actions', 'tools', 'shortcut', 'run.sh', 'stop.sh']:
        if (menu / name).exists():
            copy(menu / name, root, 'usr/share/zaman/menu/' + name)
    write(root, 'usr/bin/zaman-menu', '''#!/bin/sh
set -eu
export PATH=/usr/libexec/zaman:/usr/bin:/bin
export ZAMAN_MENU_QML=/usr/share/zaman/menu/qml/shell.qml
export ZAMAN_TRANSFER_CLIENT=/usr/libexec/zaman/transfer/zaman_transfer_client.py
export ZAMAN_NOTIFICATION_CLIENT=/usr/share/zaman/menu/tools/zaman-notifications
exec /bin/sh /usr/share/zaman/menu/run.sh /usr/share/zaman/sessiond "${1:-open}"
''', 0o755)
    packages.append(build_package(root, 'zaman-menu', v, 'all',
        f'zaman-sessiond (= {v}), zaman-helpers (= {v}), zaman-quickshell-runtime (>= 0.3.0), python3, python3-gi, gir1.2-glib-2.0, pipewire, wireplumber',
        'Zaman resident menu, notification server and shortcuts'))

    root = new_package('zaman-helpers')
    for p in (core / 'tools').iterdir():
        if p.is_file() and (p.suffix == '.py' or p.name in {'zaman-toasts', 'zaman-steam'}) and not p.name.startswith('test-'):
            copy(p, root, 'usr/libexec/zaman/' + p.name)
    if (core / 'tools/transfer/receiver').is_dir():
        copy(core / 'tools/transfer/receiver', root, 'usr/libexec/zaman/transfer')
    # Device exports preserve helpers outside repositories, including current
    # storage/artwork/theme actions. Keep directory structure for relative imports.
    live = ROOT / 'packaging/device-source'
    device_map = {}
    relocation = json.loads((WORK / 'relocation.json').read_text()) if (WORK / 'relocation.json').exists() else {}
    relocation_pattern = re.compile('|'.join(re.escape(key) for key in sorted(relocation, key=len, reverse=True))) if relocation else None
    if (live / 'deployment').is_dir():
        manifest = json.loads((live / 'manifest.json').read_text())
        active_roots = {str(Path(x).parent) for x in manifest.get('active_helpers', [])}
        for p in (live / 'deployment').rglob('*'):
            rel = p.relative_to(live / 'deployment')
            if p.is_file() and ('zaman' in str(rel).lower()) and p.suffix in {'.py', '.sh', '.js', '.qml', '.json', '.desktop'}:
                dest = 'usr/libexec/zaman/device/' + str(rel)
                if p.suffix == '.desktop':
                    dest = 'usr/share/applications/' + p.name
                copy(p, root, dest)
                content = (root / dest).read_text()
                if relocation_pattern:
                    content = relocation_pattern.sub(lambda match: relocation[match.group()], content)
                (root / dest).write_text(content)
                device_map['/' + str(rel)] = '/' + dest
                # Installed top-level helpers are authoritative over older copies
                # left in a source checkout. Relative imports retain their layout.
                if str(rel.parent) in {'usr/local/libexec/zaman', 'usr/libexec/zaman'}:
                    copy(root / dest, root, 'usr/libexec/zaman/' + p.name)
                archive_parent = 'deployment/' + str(rel.parent)
                if archive_parent in active_roots and (p.parent / 'zaman_receiver.py').is_file() and p.suffix == '.py':
                    copy(root / dest, root, 'usr/libexec/zaman/transfer/' + p.name)
        write(root, 'usr/share/zaman/device-helper-map.json', json.dumps(device_map, indent=2))
    write(root, 'usr/libexec/zaman/start-transfer', '''#!/bin/sh
set -eu
state=${XDG_STATE_HOME:-$HOME/.local/state}/zaman/transfer
roms=${ZAMAN_ROM_ROOT:-$HOME/Games/roms}
mkdir -p "$state" "$roms"
exec /usr/bin/python3 /usr/libexec/zaman/transfer/zaman_receiver.py --state "$state" --rom-root "$roms"
''', 0o755)
    packages.append(build_package(root, 'zaman-helpers', v, 'all',
        'python3, python3-gi, gir1.2-glib-2.0, dbus-user-session',
        'Zaman transfer, library, notification-event and shared helpers', 'flatpak, xdg-utils'))

    root = new_package('zaman-pegasus-theme')
    for p in theme.iterdir():
        if p.name not in {'.git', 'tests', 'preview', 'library-refresh', 'rom-library-refresh', 'steam-library-refresh'}:
            copy(p, root, 'usr/share/pegasus-frontend/themes/zaman/' + p.name)
    write(root, 'usr/share/pegasus-frontend/themes/zaman/.zaman-package-version', v)
    packages.append(build_package(root, 'zaman-pegasus-theme', v, 'all',
        'python3', 'Zaman Pegasus theme and assets', 'pegasus-frontend'))

    root = new_package('zaman-session')
    for name in ['zaman-start', 'zaman-session-enter', 'zaman-session-setup', 'zaman-library']:
        copy(ROOT / 'packaging/cm2' / name, root, 'usr/bin/' + name)
        (root / 'usr/bin' / name).chmod(0o755)
    copy(ROOT / 'packaging/cm2/sway.conf', root, 'etc/zaman/sway.conf')
    write(root, 'DEBIAN/conffiles', '/etc/zaman/sway.conf')
    units = {
        'zaman-sessiond.service': ('Zaman session daemon', '/usr/libexec/zaman/zaman-sessiond', '', 'simple'),
        'zaman-quickshell.service': ('Zaman resident menu', '/usr/bin/zaman-menu --resident', 'After=zaman-sessiond.service zaman-notifications.service\n', 'simple'),
        'zaman-notifications.service': ('Zaman notification service', '/bin/sh /usr/share/zaman/menu/tools/start-notifications.sh', '', 'dbus'),
        'zaman-menu-shortcut.service': ('Zaman controller menu shortcut', '/usr/bin/python3 /usr/share/zaman/menu/shortcut/zaman-menu-shortcut.py', 'After=zaman-quickshell.service\n', 'simple'),
        'zaman-notification-events.service': ('Zaman system notification events', '/usr/bin/python3 /usr/libexec/zaman/zaman-notification-events.py', 'After=zaman-notifications.service\n', 'simple'),
        'zaman-transfer.service': ('Zaman transfer receiver', '/usr/libexec/zaman/start-transfer', '', 'simple'),
        'zaman-library.service': ('Zaman Pegasus library', '/usr/bin/zaman-library', 'After=zaman-sessiond.service\n', 'simple'),
    }
    environment = '''Environment=PATH=/usr/libexec/zaman:/usr/bin:/bin
Environment=QT_QPA_PLATFORM=wayland
Environment=ZAMAN_MENU_QML=/usr/share/zaman/menu/qml/shell.qml
Environment=ZAMAN_MENU_LAUNCHER=/usr/share/zaman/menu/run.sh
Environment=ZAMAN_SESSIOND_PROJECT=/usr/share/zaman/sessiond
Environment=ZAMAN_TRANSFER_CLIENT=/usr/libexec/zaman/transfer/zaman_transfer_client.py
Environment=ZAMAN_NOTIFICATION_CLIENT=/usr/share/zaman/menu/tools/zaman-notifications
Environment=ZAMAN_PEGASUS_LIBRARY_SYNC_HELPER=/usr/libexec/zaman/pegasus-library-sync.py
Environment=ZAMAN_INSTALLED_GAME_HELPER=/usr/libexec/zaman/flatpak-game-session.py
Environment=ZAMAN_CATALOG_PATH=/usr/share/zaman/catalog.json
EnvironmentFile=-/etc/zaman/session.env
'''
    for name, (desc, cmd, after, kind) in units.items():
        extra = 'BusName=org.freedesktop.Notifications\n' if kind == 'dbus' else ''
        write(root, 'usr/lib/systemd/user/' + name, f'''[Unit]
Description={desc}
PartOf=zaman-session.target
{after}
[Service]
Type={kind}
{extra}ExecStart={cmd}
{environment}Restart=on-failure
RestartSec=3
TimeoutStopSec=25
KillMode=mixed
''')
    write(root, 'usr/lib/systemd/user/zaman-session.target', '''[Unit]
Description=Zaman graphical session
Wants=zaman-sessiond.service zaman-quickshell.service zaman-notifications.service zaman-menu-shortcut.service zaman-notification-events.service zaman-transfer.service zaman-library.service
After=graphical-session-pre.target
''')
    write(root, 'usr/share/wayland-sessions/zaman.desktop', '''[Desktop Entry]
Name=Zaman
Comment=Zaman console session
Exec=/usr/bin/zaman-start
Type=Application
DesktopNames=Zaman;sway
''')
    packages.append(build_package(root, 'zaman-session', v, 'all',
        f'zaman-sessiond (= {v}), zaman-menu (= {v}), zaman-helpers (= {v}), zaman-pegasus-theme (= {v}), sway, swaybg, foot, dbus-user-session, python3, pipewire, pipewire-pulse, wireplumber, xdg-utils',
        'Zaman Sway session, services and first-run setup', 'inputplumber, pegasus-frontend, network-manager, bluez, xwayland'))

    root = new_package('zaman-stack')
    packages.append(build_package(root, 'zaman-stack', v, 'all', f'zaman-session (= {v})', 'Zaman component metapackage'))
    report = [{'file': p.name, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in packages]
    (OUT / 'components.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
