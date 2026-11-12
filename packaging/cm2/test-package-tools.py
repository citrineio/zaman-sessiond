#!/usr/bin/env python3
"""Checks release completeness, source-path handling and package file ownership."""
import importlib.util
import json
import subprocess
import unittest
from pathlib import Path


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PackageTools(unittest.TestCase):
    def test_complete_release(self):
        installer = load('installer', 'install.py')
        release = {'assets': [{'name': name + '_1.0_' + ('arm64' if name in {'zaman-sessiond', 'zaman-quickshell-runtime'} else 'all') + '.deb'} for name in installer.REQUIRED]}
        self.assertEqual(set(installer.select_assets(release)), installer.REQUIRED)
        release['assets'].pop()
        with self.assertRaisesRegex(ValueError, 'Incomplete'):
            installer.select_assets(release)

    def test_duplicate_release_rejected(self):
        installer = load('installer2', 'install.py')
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            installer.select_assets({'assets': [{'name': 'zaman-sessiond_1_arm64.deb'}, {'name': 'zaman-sessiond_2_arm64.deb'}]})

    def test_archive_path_rejected(self):
        installer = load('installer3', 'install.py')
        with self.assertRaisesRegex(ValueError, 'Unsafe'):
            installer.select_assets({'assets': [{'name': '../zaman-sessiond_1_arm64.deb'}]})

    def test_resolver_rejects_ambiguous_sources(self):
        exporter = load('exporter', 'export-zaman-stack.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            roots = [Path(td) / n for n in ['first', 'second']]
            for root in roots:
                (root / 'src').mkdir(parents=True)
                (root / 'Cargo.toml').write_text('[package]')
                (root / 'src/main.rs').write_text('fn main() {}')
            with self.assertRaisesRegex(ValueError, 'candidates'):
                exporter.choose_root('sessiond', None, roots, [], ['Cargo.toml', 'src/main.rs'])

    def test_session_uses_user_bus(self):
        content = Path(__file__).with_name('zaman-start').read_text()
        self.assertIn('unix:path=$XDG_RUNTIME_DIR/bus', content)
        self.assertNotIn('dbus-run-session', content)
        self.assertIn('stop zaman-session.target', content)


if __name__ == '__main__':
    unittest.main()
