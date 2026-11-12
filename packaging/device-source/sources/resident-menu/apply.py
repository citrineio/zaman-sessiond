#!/usr/bin/env python3
"""Add development helper sources only. Does not build, install or restart."""
import argparse
import hashlib
from pathlib import Path
import shutil
from datetime import datetime, timezone

PREVIOUS_HELPERS = {
    '1d0657f0dbfee6223c7111d5602277d0684c500609768ec1f2dd1c0ce90f63e9',
    '7b305b53add2aafa70efb6f95dcdeccd2e96c7ce3dd30c16e7102c440b88642e',
    'b6fbd3413bc0ea3212aefab46a5bf65fe8281c56e4c71c37243f61c4014f27d4',
    'e76969d9b4eccaa211b1f739ac9534e8a3948d82112fd69623f79f5fc11cff5b',
    'dc3bb8e46cb5d9f27cf028ec8ed17f8add213c4a29c177c1b6ae34b73797ca0e',
    'cadc37fefa72f3c3a8aaa5dc098c2beb95898f3235355447fa700a7639d24cf1',
    '6e4062c5bb713a19aafea41ddc2f75ed3bc033e2af32ff0e1a805cce95e34850',
    '595d01556d62d79f8d3eb11244ffd6f4a8139a41f5fb2b8581ca87558478d655',
    '8abd0d7ebb4ef91a575d1fe1303bd541ec3684da3372709ab3fe5ed67350d63e',
    'b17a7828c0f6910df7a178e64631d9a4b64c3f0b720740bb899a7d45150c4db2',
    '014f7b39845773b9d1daab52fce2a5cb9b10bce0724b7f119577de6e3aa14909',
    '971faa3c2798b2f2fe7d088a7fda67d3cd55e39eff199c0ba7f2792e03a30e7b',
    '6bedb861e6b59a2a1001326ceef5ea217a0452320b4379fb77c4db9e138840d1',
}

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('project', type=Path)
args = parser.parse_args()
project = args.project.expanduser().resolve()
bundle = Path(__file__).resolve().parent
for name in ['Cargo.toml', 'Cargo.lock', 'src/error.rs', 'src/guide.rs', 'src/inputplumber.rs']:
    if not (project / name).is_file():
        parser.error('Missing project file: ' + name)
files = [
    ('zaman-menu-dev-input.rs', 'src/bin/zaman-menu-dev-input.rs'),
    ('zaman-menu-dev-pair.rs', 'src/bin/zaman-menu-dev-pair.rs'),
    ('display.rs', 'src/bin/zaman-menu-dev/display.rs'),
    ('display_bridge.rs', 'src/bin/zaman-menu-dev/display_bridge.rs'),
]
changes = []
# Validate every target before writing any file, including unknown local edits.
for relative, destination in files:
    source = bundle / 'src' / relative
    target = project / destination
    if not target.resolve().is_relative_to(project):
        parser.error('Development source resolves outside the project: ' + destination + '; nothing changed.')
    if not source.is_file():
        parser.error('Missing bundled source: ' + relative)
    if target.is_symlink() or (target.exists() and not target.is_file()):
        parser.error('Development source path is not a regular file: ' + destination + '; nothing changed.')
    if target.exists():
        existing = target.read_bytes()
        if existing == source.read_bytes():
            continue
        recognized = (relative == 'zaman-menu-dev-input.rs' and hashlib.sha256(existing).hexdigest() in PREVIOUS_HELPERS) or (
            relative == 'display_bridge.rs' and hashlib.sha256(existing).hexdigest() ==
            '0f43453118efab35404d41a489bf7fb61b2fb7b91a37b5b88cf25e19a7413af4')
        if not recognized:
            parser.error('Unrecognized development source edits: ' + destination + '; nothing changed.')
    changes.append((source, target, destination))
for source, target, destination in changes:
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        stamp = datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
        backup = target.with_name(target.name + '.pre-update-' + stamp)
        shutil.copy2(target, backup)
        print('Backup: ' + str(backup))
    shutil.copyfile(source, target)
    print('Wrote ' + destination)
if not changes:
    print('Development helper sources are already current.')
print('Cargo manifests, daemon source, installed binaries and services were not changed.')
