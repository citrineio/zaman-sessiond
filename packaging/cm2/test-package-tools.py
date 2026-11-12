#!/usr/bin/env python3
"""Checks release completeness, source-path handling and package file ownership."""
import importlib.util
import json
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch


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

    def test_discovery_skips_proc_without_stat(self):
        exporter = load('exporter_proc', 'export-zaman-stack.py')
        with patch.object(Path, 'stat', side_effect=PermissionError('restricted procfs')):
            self.assertIsNone(exporter.source_hint(Path('/proc/928/exe')))
            self.assertEqual(exporter.ancestor_candidates([Path('/proc/928/exe')], ['Cargo.toml']), [])

    def test_unreadable_hint_does_not_hide_valid_root(self):
        exporter = load('exporter_unreadable', 'export-zaman-stack.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / 'source'
            (root / 'src').mkdir(parents=True)
            (root / 'Cargo.toml').write_text('[package]')
            (root / 'src/main.rs').write_text('fn main() {}')
            original = Path.resolve
            def resolve(path, *args, **kwargs):
                if str(path) == '/restricted/source':
                    raise PermissionError('denied')
                return original(path, *args, **kwargs)
            with patch.object(Path, 'resolve', resolve):
                self.assertEqual(exporter.ancestor_candidates([Path('/restricted/source'), root],
                                 ['Cargo.toml', 'src/main.rs']), [root.resolve()])

    def test_service_source_used_when_process_has_no_source(self):
        exporter = load('exporter_service', 'export-zaman-stack.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / 'source'
            (root / 'src').mkdir(parents=True)
            (root / 'Cargo.toml').write_text('[package]')
            (root / 'src/main.rs').write_text('fn main() {}')
            found = exporter.choose_root('sessiond', None, [Path('/usr/bin/python3')], [],
                                         ['Cargo.toml', 'src/main.rs'], secondary_hints=[root])
            self.assertEqual(found, root.resolve())

    def test_denied_executable_still_checks_command_arguments(self):
        exporter = load('exporter_process', 'export-zaman-stack.py')
        import tempfile
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as td:
            qml = Path(td) / 'shell.qml'
            qml.write_text('import QtQuick')
            proc = Path('/proc/928')
            original_stat, original_read = Path.stat, Path.read_bytes
            def stat(path, *args, **kwargs):
                if path == proc:
                    return SimpleNamespace(st_uid=exporter.os.getuid())
                return original_stat(path, *args, **kwargs)
            def read(path, *args, **kwargs):
                if path == proc / 'cmdline':
                    return b'quickshell\0-p\0' + str(qml).encode() + b'\0'
                return original_read(path, *args, **kwargs)
            with patch.object(Path, 'glob', return_value=iter([proc])), \
                 patch.object(Path, 'stat', stat), patch.object(Path, 'read_bytes', read), \
                 patch.object(Path, 'readlink', side_effect=PermissionError('denied')):
                self.assertIn(qml.resolve(), exporter.proc_paths())

    def test_daemon_unit_wins_over_other_active_helper_checkout(self):
        exporter = load('exporter_split_daemon', 'export-zaman-stack.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            daemon, helper = [Path(td) / x for x in ['sprint4', 'sprint7']]
            for root in [daemon, helper]:
                (root / 'src').mkdir(parents=True)
                (root / 'Cargo.toml').write_text('[package]')
                (root / 'src/main.rs').write_text('fn main() {}')
            record = {'stdout': 'MainPID=0\nExecStart={ path=' + str(daemon) + '/target/release/zaman-sessiond ; }\nWorkingDirectory=' + str(daemon)}
            # An installed executable may be unreadable or outside its source tree;
            # WorkingDirectory/ExecStart still belong to this particular unit.
            selected = exporter.choose_root('sessiond', None, [daemon, helper], [],
                ['Cargo.toml', 'src/main.rs'], configured_hints=exporter.unit_paths(record))
            self.assertEqual(selected, daemon.resolve())
            selected = exporter.choose_root('sessiond', None, [daemon, helper], [],
                ['Cargo.toml', 'src/main.rs'], preferred_hints=[helper],
                configured_hints=exporter.unit_paths(record))
            self.assertEqual(selected, helper.resolve())

    def test_helper_checkout_is_captured_and_built_separately(self):
        exporter = load('exporter_split_helper', 'export-zaman-stack.py')
        builder = load('builder_split_helper', 'build-targets.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            daemon, helper = [Path(td) / x for x in ['sprint4', 'sprint7']]
            for root in [daemon, helper]:
                (root / 'src/bin').mkdir(parents=True)
                (root / 'target/release').mkdir(parents=True)
                (root / 'Cargo.toml').write_text('[package]')
                for name in ['zaman-menu-dev-input', 'zaman-menu-dev-pair']:
                    (root / 'src/bin' / (name + '.rs')).write_text('fn main() {}')
                    (root / 'target/release' / name).write_text('binary fixture')
            record = {'stdout': 'MainPID=0\nExecStart=/bin/sh /nonexistent/menu/run.sh ' + str(helper) + ' --resident'}
            projects, selected = exporter.helper_projects(record,
                [helper / 'target/release/zaman-menu-dev-input'], daemon)
            self.assertEqual(len(projects), 1)
            self.assertEqual(set(selected), {'zaman-menu-dev-input', 'zaman-menu-dev-pair'})
            identity = next(iter(projects))
            self.assertEqual(projects[identity]['root'], helper.resolve())
            work = Path(td) / 'work'
            work.mkdir()
            (work / 'helper-builds.json').write_text(json.dumps(selected))
            plan = builder.build_plan(work)
            self.assertEqual(plan['core'], ['zaman-sessiond', 'zamanctl'])
            self.assertEqual(plan['helper-projects/' + identity], ['zaman-menu-dev-input', 'zaman-menu-dev-pair'])

    def test_unit_main_pid_paths_are_used(self):
        exporter = load('exporter_main_pid', 'export-zaman-stack.py')
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            executable = Path(td) / 'zaman-sessiond'
            executable.write_text('fixture')
            record = {'stdout': 'MainPID=222\nActiveState=active'}
            with patch.object(Path, 'readlink', return_value=executable), \
                 patch.object(Path, 'read_bytes', return_value=str(executable).encode() + b'\0'):
                self.assertIn(executable.resolve(), exporter.unit_process_paths(record))


if __name__ == '__main__':
    unittest.main()
