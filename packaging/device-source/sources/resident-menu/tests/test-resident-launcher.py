#!/usr/bin/env python3
"""Exercise actual launch/stop scripts with mocked external programs."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

BUNDLE = Path(__file__).resolve().parent.parent


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.proto = self.root / 'prototype'
        self.proto.mkdir()
        for name in ('run.sh', 'stop.sh'):
            shutil.copy2(BUNDLE / name, self.proto / name)
        (self.proto / 'qml').mkdir()
        (self.proto / 'qml/shell.qml').write_text('fixture')
        self.project = self.root / 'project'
        helper = self.project / 'target/release/zaman-menu-dev-input'
        helper.parent.mkdir(parents=True)
        helper.write_text('#!/bin/sh\n# zaman-menu-helper-resident-v1\nexit 0\n')
        helper.chmod(0o755)
        commands = self.root / 'commands'
        commands.mkdir()
        self.log = self.root / 'calls.jsonl'
        mock = '''#!/usr/bin/python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
with open(os.environ['TEST_LOG'], 'a') as out:
    out.write(json.dumps([name, sys.argv[1:], os.environ.get('ZAMAN_MENU_DEV_OPEN_ON_START')]) + '\\n')
if name == 'systemctl':
    if 'show-environment' in sys.argv:
        print('SWAYSOCK=/test/sway.sock\\nWAYLAND_DISPLAY=wayland-test\\nXDG_RUNTIME_DIR=/test/runtime')
elif name == 'qs':
    if 'ipc' not in sys.argv: sys.exit(0)
    counter = pathlib.Path(os.environ['TEST_COUNTER'])
    count = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(count+1))
    if count < int(os.environ.get('TEST_FAIL_UNTIL', '0')): sys.exit(1)
    if sys.argv[-1] == 'status':
        print('closing' if count < 3 else 'hidden')
    else: print('opening' if sys.argv[-1] == 'open' else 'closing')
'''
        for name in ('systemctl', 'qs'):
            program = commands / name
            program.write_text(mock)
            program.chmod(0o755)
        shell = commands / 'sh'
        shell.write_text('#!/bin/bash\nexec /bin/bash "$@"\n')
        shell.chmod(0o755)
        # Only emulate the unavailable socket predicate. Actual shell routing runs.
        bootstrap = self.root / 'bash-env'
        bootstrap.write_text('''function [() {
if [[ "$#" = 3 && "$1" = -S && "$2" = /test/sway.sock ]]; then return 0; fi
if [[ "$#" = 4 && "$1" = ! && "$2" = -S && "$3" = /test/sway.sock ]]; then return 1; fi
builtin [ "$@"
}
export -f '['
''')
        self.env = dict(os.environ, PATH=str(commands)+':'+os.environ['PATH'],
            BASH_ENV=str(bootstrap), TEST_LOG=str(self.log),
            TEST_COUNTER=str(self.root / 'counter'), HOME=str(self.root),
            SWAYSOCK='', WAYLAND_DISPLAY='', XDG_RUNTIME_DIR='')
        self.env.pop('QS_CONFIG_PATH', None)

    def tearDown(self):
        self.directory.cleanup()

    def run_script(self, script='run.sh', mode=None):
        command = ['bash', str(self.proto / script), str(self.project)]
        if mode: command.append(mode)
        return subprocess.run(command, env=self.env, capture_output=True, text=True, timeout=5)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_resident_boot_launches_qs_hidden_without_ipc_or_helper(self):
        result = self.run_script(mode='--resident')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        qs = [call for call in calls if call[0] == 'qs']
        self.assertEqual(qs, [['qs', ['--no-duplicate', '-p', str(self.proto / 'qml/shell.qml')], '0']])
        self.assertFalse(any('start' in call[1] for call in calls))

    def test_warm_open_uses_ipc_without_starting_or_replacing_service(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'opening')
        qs = [call for call in self.calls() if call[0] == 'qs']
        self.assertEqual(qs[0][1][-4:], ['ipc', 'call', 'menu', 'open'])
        self.assertEqual(len(qs), 1)
        self.assertFalse(any('start' in call[1] for call in self.calls()))

    def test_absent_renderer_starts_service_then_retries_open(self):
        self.env['TEST_FAIL_UNTIL'] = '1'
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(['systemctl', ['--user', 'start', 'zaman-quickshell.service'], None], self.calls())
        self.assertEqual(len([call for call in self.calls() if call[0] == 'qs']), 2)

    def test_status_does_not_start_an_absent_renderer(self):
        self.env['TEST_FAIL_UNTIL'] = '10'
        result = self.run_script(mode='status')
        self.assertEqual(result.returncode, 1)
        self.assertFalse(any('start' in call[1] for call in self.calls()))

    def test_timings_uses_ipc_without_starting_services(self):
        result = self.run_script(mode='timings')
        self.assertEqual(result.returncode, 0, result.stderr)
        qs = [call for call in self.calls() if call[0] == 'qs']
        self.assertEqual(qs[0][1][-4:], ['ipc', 'call', 'menu', 'timings'])
        self.assertFalse(any('start' in call[1] for call in self.calls()))

    def test_old_binary_is_rejected_without_executing_it(self):
        helper = self.project / 'target/release/zaman-menu-dev-input'
        helper.write_text('#!/bin/sh\necho old-helper-executed >&2\nexit 0\n')
        result = self.run_script(mode='--resident')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Rebuild', result.stderr)
        self.assertNotIn('old-helper-executed', result.stderr)
        self.assertFalse(any(call[0] == 'qs' for call in self.calls()))

    def test_invalid_mode_refuses_before_contacting_services(self):
        result = self.run_script(mode='invalid')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(self.calls(), [])

    def test_stop_waits_for_hidden_after_close_without_starting_service(self):
        result = self.run_script(script='stop.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        qs = [call for call in self.calls() if call[0] == 'qs']
        self.assertEqual([call[1][-1] for call in qs], ['close', 'status', 'status', 'status'])
        self.assertFalse(any('start' in call[1] for call in self.calls()))


if __name__ == '__main__':
    unittest.main()
