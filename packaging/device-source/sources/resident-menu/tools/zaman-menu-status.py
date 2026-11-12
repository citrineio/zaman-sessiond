#!/usr/bin/env python3
"""Read-only storage inventory and background-job adapter for the Zaman menu."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SYSTEM_MOUNTS = ('/boot', '/efi', '/usr', '/var', '/opt', '/etc', '/recovery')
SYSTEM_PARTITIONS = {'c12a7328-f81f-11d2-ba4b-00a0c93ec93b',
                     '21686148-6449-6e6f-744e-656564454649', '0xef'}
FILESYSTEMS = {'ext2', 'ext3', 'ext4', 'exfat', 'vfat', 'ntfs', 'ntfs3',
               'btrfs', 'xfs', 'f2fs', 'udf', 'iso9660'}


def clean(value, limit=160):
    return re.sub(r'[\x00-\x1f\x7f]', '', str(value or ''))[:limit]


def number(value):
    try:
        value = float(value)
        return value if math.isfinite(value) and value >= 0 else None
    except (ValueError, TypeError):
        return None


def children(node):
    yield node
    for child in node.get('children') or []:
        yield from children(child)


def device_type(disk, sysroot=Path('/sys/class/block')):
    name = str(disk.get('kname') or disk.get('name') or '')
    transport = clean(disk.get('tran')).lower()
    model = clean(disk.get('model'))
    try:
        path = str((sysroot / name / 'device').resolve()).lower()
    except OSError:
        path = ''
    if name.startswith('mmcblk'):
        try:
            kind = (sysroot / name / 'device/type').read_text().strip()
        except OSError:
            kind = ''
        if kind == 'SD' or (not kind and disk.get('rm') in (True, 1, '1')):
            return 'SD card', 'SD'
        return ('eMMC' if kind == 'MMC' else 'MMC storage'), 'MMC'
    if 'ufs' in path:
        return 'UFS', 'Internal'
    if name.startswith('nvme'):
        return 'SSD', 'NVMe'
    if transport == 'usb':
        if disk.get('rota') in (True, 1, '1'):
            return 'Hard drive', 'USB'
        # Nonrotating USB devices include flash drives and card readers as well as SSDs.
        return ('SSD' if re.search(r'\bSSD\b', model, re.I) else 'USB storage'), 'USB'
    if disk.get('rota') in (True, 1, '1'):
        return 'Hard drive', transport.upper() or 'Internal'
    if transport in ('sata', 'ata'):
        return 'SSD', transport.upper()
    return 'Solid-state storage', transport.upper() or 'Internal'


def usable_mount(path):
    return path == '/' or not (path in SYSTEM_MOUNTS or path.startswith('/boot/') or path.startswith('/efi/'))


def storage_inventory(tree, statvfs=os.statvfs, sysroot=Path('/sys/class/block')):
    devices = []
    seen_devices, seen_volumes = set(), set()
    for disk in tree.get('blockdevices') or []:
        name = str(disk.get('kname') or disk.get('name') or '')
        disk_path = str(disk.get('path') or '')
        if disk.get('type') != 'disk' or re.search(r'(^loop|^zram|boot\d+$|rpmb$)', name):
            continue
        if not disk_path.startswith('/dev/') or disk_path in seen_devices:
            continue
        volumes = []
        for node in children(disk):
            path = str(node.get('path') or '')
            filesystem = str(node.get('fstype') or '').lower()
            if path in seen_volumes or not path.startswith('/dev/') or filesystem not in FILESYSTEMS:
                continue
            if str(node.get('parttype') or '').lower() in SYSTEM_PARTITIONS:
                continue
            mounts = [m for m in (node.get('mountpoints') or []) if isinstance(m, str) and m]
            mounts = [m for m in mounts if usable_mount(m)]
            if not mounts and any(node.get('mountpoints') or []):
                continue
            label = clean(node.get('label'))
            if not mounts and label.lower() in ('boot', 'bootfs', 'efi', 'esp', 'system-boot', 'recovery'):
                continue
            size = int(number(node.get('size')) or 0)
            if size <= 0:
                continue
            volume = dict(id=path, name=label or Path(path).name, filesystem=filesystem,
                          uuid=clean(node.get('uuid')), capacity=size, mountpoint=mounts[0] if mounts else '',
                          total=None, used=None, available=None, reserved=None,
                          readOnly=node.get('ro') in (True, 1, '1'))
            for mount in mounts:
                try:
                    stat = statvfs(mount)
                    total = stat.f_blocks * stat.f_frsize
                    used = max(0, stat.f_blocks - stat.f_bfree) * stat.f_frsize
                    available = max(0, stat.f_bavail) * stat.f_frsize
                    volume.update(total=total, used=used, available=available,
                                  reserved=max(0, total - used - available), mountpoint=mount,
                                  readOnly=volume['readOnly'] or bool(stat.f_flag & getattr(os, 'ST_RDONLY', 1)))
                    break
                except OSError:
                    continue
            seen_volumes.add(path)
            volumes.append(volume)
        if not volumes:
            continue
        seen_devices.add(disk_path)
        kind, connection = device_type(disk, sysroot)
        internal = any(v['mountpoint'] == '/' for v in volumes)
        model = clean(disk.get('model'))
        title = 'Internal storage' if internal else next((v['name'] for v in volumes if v['uuid']), model or kind)
        devices.append(dict(id=disk_path, name=title, model=model, type=kind, connection=connection,
                            physicalSize=int(number(disk.get('size')) or 0), volumes=volumes,
                            internal=internal))
    return dict(type='storage', available=True, devices=sorted(devices, key=lambda d: (not d['internal'], d['id'])), error='')


def storage():
    columns = 'NAME,KNAME,PATH,TYPE,SIZE,ROTA,RM,TRAN,FSTYPE,LABEL,UUID,MOUNTPOINTS,RO,MODEL,PARTTYPE'
    result = subprocess.run(['lsblk', '--json', '--bytes', '--output', columns],
                            capture_output=True, text=True, timeout=5, check=True)
    return storage_inventory(json.loads(result.stdout))


def clients():
    try:
        config = json.loads((ROOT / 'demo-update-runtime.json').read_text())
    except (OSError, ValueError):
        config = {}
    return {name: os.environ.get(variable) or config.get(name, '') for name, variable in
            [('transfer', 'ZAMAN_TRANSFER_CLIENT'), ('transactions', 'DUKKAN_TRANSACTION_CLIENT')]}


def snapshot(path, expected):
    if not isinstance(path, str) or not Path(path).is_absolute() or not Path(path).is_file():
        return None
    try:
        result = subprocess.run(['/usr/bin/python3', path, 'snapshot'], capture_output=True,
                                text=True, timeout=3, check=True)
        for line in result.stdout.splitlines():
            data = json.loads(line)
            if isinstance(data, dict) and data.get('type') == expected:
                return data
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    return None


def background_jobs(transfer, transactions):
    jobs, terminal = [], {}
    imports = (transfer or {}).get('jobs') or []
    scrape_refs = {str(j.get('scrape_job')) for j in imports if j.get('scrape_job')}
    names = {'offered': 'Waiting for sender', 'receiving': 'Receiving game', 'validating': 'Checking game',
             'needs-scrape': 'Preparing artwork', 'scraping': 'Finding artwork & info',
             'ready': 'Waiting to update library', 'importing': 'Adding to library',
             'needs-destination': 'Choose a game folder', 'needs-files': 'Transfer needs attention'}
    for job in imports:
        phase = job.get('phase')
        identity = 'transfer:' + str(job.get('id', ''))
        if phase in ('complete', 'failed', 'cancelled'):
            terminal[identity] = phase
        elif phase in names:
            received, size = number(job.get('received')), number(job.get('size'))
            percent = min(1, received / size) if phase == 'receiving' and received is not None and size else None
            jobs.append(dict(id=identity, title=clean(job.get('name')) or 'Game transfer',
                             stage=names[phase], category='transfer', progress=percent,
                             waiting=phase in ('offered', 'ready', 'needs-destination', 'needs-files')))
    for job in ((transfer or {}).get('scraper') or {}).get('jobs') or []:
        identity = 'artwork:' + str(job.get('id', ''))
        phase = job.get('phase')
        if phase in ('complete', 'failed', 'cancelled', 'interrupted'):
            terminal[identity] = phase
        elif str(job.get('id')) not in scrape_refs and phase in ('queued', 'running', 'needs-input', 'waiting-library', 'publishing'):
            value = number(job.get('progress'))
            jobs.append(dict(id=identity, title='Artwork & Game Info', stage=clean(job.get('stage')) or 'Updating artwork',
                             category='scrape', progress=min(1, value) if value is not None and phase == 'running' else None,
                             waiting=phase in ('queued', 'needs-input', 'waiting-library')))
    for job in (transactions or {}).get('jobs') or []:
        phase = job.get('phase')
        identity = 'install:' + str(job.get('id', ''))
        if phase in ('complete', 'completed', 'succeeded', 'failed', 'cancelled'):
            terminal[identity] = phase
        elif phase in ('queued', 'waiting', 'running', 'cancelling'):
            value = number(job.get('percent'))
            jobs.append(dict(id=identity, title=clean(job.get('name') or job.get('appId')) or 'Game installation',
                             stage=clean(job.get('stage')) or 'Installing game', category='install',
                             progress=min(1, value / 100) if value is not None and phase == 'running' else None,
                             waiting=phase in ('queued', 'waiting')))
    # Active work takes precedence over jobs awaiting user action. Cap IPC size.
    return sorted(jobs, key=lambda j: (j['waiting'], j['id']))[:100], terminal


def background_transition(previous, jobs, terminal):
    finished = [dict(id=key, phase=terminal[key]) for key in sorted(previous) if key in terminal]
    # A missing producer is not a successful completion of its last known job.
    if not jobs and any(key not in terminal for key in previous):
        finished = []
    return finished, {job['id'] for job in jobs}


def watch():
    previous = set()
    with ThreadPoolExecutor(max_workers=2) as pool:
        while True:
            paths = clients()
            transfer_future = pool.submit(snapshot, paths['transfer'], 'transfer')
            transaction_future = pool.submit(snapshot, paths['transactions'], 'transactions')
            jobs, terminal = background_jobs(transfer_future.result(), transaction_future.result())
            finished, previous = background_transition(previous, jobs, terminal)
            print(json.dumps(dict(type='background', jobs=jobs, finished=finished), ensure_ascii=False), flush=True)
            time.sleep(2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('storage', 'watch', 'clients'))
    args = parser.parse_args()
    if args.action == 'watch':
        watch()
    elif args.action == 'clients':
        print(json.dumps(clients()))
    else:
        try:
            data = storage()
        except (OSError, ValueError, subprocess.SubprocessError):
            data = dict(type='storage', available=False, devices=[], error='Storage inventory is unavailable. Select Refresh to retry.')
        print(json.dumps(data, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    try:
        main()
    except (BrokenPipeError, KeyboardInterrupt):
        pass
