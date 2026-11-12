#!/usr/bin/env python3
"""Build daemon and live helpers from their respective exported Rust projects."""
import json
import os
import shutil
import subprocess
from pathlib import Path


def build_plan(work):
    selected = json.loads((work / 'helper-builds.json').read_text()) if (work / 'helper-builds.json').exists() else {}
    projects = {'core': ['zaman-sessiond', 'zamanctl']}
    for binary in ['zaman-menu-dev-input', 'zaman-menu-dev-pair']:
        identity = selected.get(binary)
        project = 'helper-projects/' + identity if identity else 'core'
        if identity and (len(identity) != 12 or any(c not in '0123456789abcdef' for c in identity)):
            raise ValueError('Invalid helper project identity')
        projects.setdefault(project, []).append(binary)
    return projects


def main():
    root = Path(__file__).resolve().parents[2]
    work = root / 'packaging/work'
    destination = work / 'core/target/release'
    destination.mkdir(parents=True, exist_ok=True)
    for project, binaries in build_plan(work).items():
        source = work / project
        target = work / 'rust-targets' / project.replace('/', '-')
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        subprocess.run(['cargo', 'test', '--locked', '--lib'], cwd=source, env=env, check=True)
        command = ['cargo', 'build', '--release', '--locked']
        for binary in binaries:
            command += ['--bin', binary]
        subprocess.run(command, cwd=source, env=env, check=True)
        for binary in binaries:
            shutil.copy2(target / 'release' / binary, destination / binary)


if __name__ == '__main__':
    main()
