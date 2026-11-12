#!/usr/bin/env python3
"""Download, verify, and install a complete Zaman CM2 component release."""
import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import tempfile
import urllib.request
from pathlib import Path

REPO = 'citrineio/zaman-sessiond'
PREFIX = 'cm2-bookworm-'
REQUIRED = {'zaman-sessiond', 'zaman-menu', 'zaman-helpers', 'zaman-pegasus-theme',
            'zaman-session', 'zaman-stack', 'zaman-quickshell-runtime'}


def get(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'zaman-package-installer'})
    with urllib.request.urlopen(request, timeout=90) as response:
        return response.read()


def select_assets(release):
    found = {}
    for asset in release['assets']:
        name = asset['name']
        if Path(name).name != name:
            raise ValueError('Unsafe asset name')
        package = name.split('_')[0]
        if name.endswith('.deb') and package in REQUIRED and (name.endswith('_arm64.deb') or name.endswith('_all.deb')):
            if package in found:
                raise ValueError('Duplicate package asset: ' + package)
            found[package] = asset
    missing = REQUIRED - set(found)
    if missing:
        raise ValueError('Incomplete release, missing: ' + ', '.join(sorted(missing)))
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--tag', help='Exact cm2-bookworm release tag; defaults to newest complete published release')
    ap.add_argument('--download-only', type=Path, help='Save verified packages without installing')
    args = ap.parse_args()
    arch = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
    if arch != 'arm64':
        ap.error('This release targets ARM64. Run this installer on the CM2.')
    os_release = {}
    for line in Path('/etc/os-release').read_text().splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            os_release[key] = value.strip('"')
    if os_release.get('VERSION_CODENAME') != 'bookworm':
        ap.error('This installer targets Debian 12 / Bookworm userspace.')
    if args.tag:
        if not re.fullmatch(r'cm2-bookworm-[A-Za-z0-9.+_-]+', args.tag):
            ap.error('Invalid release tag')
        release = json.loads(get('https://api.github.com/repos/' + REPO + '/releases/tags/' + args.tag))
        assets = select_assets(release)
    else:
        releases = json.loads(get('https://api.github.com/repos/' + REPO + '/releases?per_page=100'))
        release = None
        for candidate in releases:
            if not candidate['draft'] and candidate['tag_name'].startswith(PREFIX):
                try:
                    assets = select_assets(candidate)
                except ValueError:
                    continue
                release = candidate
                break
        if release is None:
            raise SystemExit('No complete CM2 package release is available yet. Check GitHub Actions.')
    print('Release:', release['html_url'], flush=True)
    checksum_asset = next(a for a in release['assets'] if a['name'] == 'SHA256SUMS')
    checksums = {}
    for line in get(checksum_asset['browser_download_url']).decode().splitlines():
        checksum, name = line.split(maxsplit=1)
        checksums[name.lstrip('*')] = checksum
    destination = args.download_only.expanduser().absolute() if args.download_only else Path(tempfile.mkdtemp(prefix='zaman-install-'))
    destination.mkdir(parents=True, exist_ok=True)
    packages = []
    try:
        for name, asset in sorted(assets.items()):
            print('Downloading:', asset['name'], flush=True)
            path = destination / asset['name']
            digest = hashlib.sha256()
            with urllib.request.urlopen(asset['browser_download_url'], timeout=90) as response, path.open('wb') as output:
                for chunk in iter(lambda: response.read(1024 * 1024), b''):
                    output.write(chunk)
                    digest.update(chunk)
            if digest.hexdigest() != checksums.get(asset['name']):
                raise RuntimeError('Checksum mismatch: ' + asset['name'])
            actual = subprocess.check_output(['dpkg-deb', '-f', str(path), 'Package'], text=True).strip()
            if actual != name:
                raise RuntimeError('Unexpected package metadata: ' + asset['name'])
            packages.append(str(path))
        if args.download_only:
            print('Verified packages:', destination)
            return
        sudo = [] if os.geteuid() == 0 else ['sudo']
        subprocess.run([*sudo, 'apt-get', 'update'], check=True)
        # apt presents the actual transaction for confirmation. Installation does
        # not enable a kiosk login or replace the vendor boot/GPU configuration.
        subprocess.run([*sudo, 'apt-get', 'install', '--no-install-recommends', *packages], check=True)
        print('Installed. From a local text console, run: zaman-start')
        print('Pegasus application and InputPlumber are separate dependencies; F12 opens the menu for bench testing.')
    finally:
        if not args.download_only:
            import shutil
            shutil.rmtree(destination)


if __name__ == '__main__':
    main()
