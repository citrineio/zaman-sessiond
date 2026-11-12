import contextlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('integration', Path(__file__).with_name('steam-integration.py'))
integration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(integration)

APP = '8765432'


class Clock:
    now = 1000
    def monotonic(self): return self.now
    def sleep(self, duration): self.now += duration


class Sender:
    pid = 9000000
    returncode = 0
    def poll(self): return self.returncode


class Driver:
    def __init__(self):
        self.actions = []
        self.sent = False
        self.at_home = False
        self.samples = 0
        self.broken = False
    def action(self, name, generation):
        self.actions.append((name, generation))
        if name == 'home': self.at_home = True
    def store(self):
        return ('', 0, 'Idle', 'zaman', False, '') if self.at_home else ('steam', 9, 'Running', 'steam', False, '')
    def unit(self):
        if self.broken and self.sent:
            raise RuntimeError('Client invocation changed')
        return ('invocation', '/group', 1, 10)
    def call(self, method):
        if method == 'MenuStatus': return (False, 0, '')
        if method == 'ForegroundStatus': return ('library',)
        raise AssertionError(method)
    def snapshot(self, group):
        playing = False
        if self.sent:
            self.samples += 1
            playing = self.samples <= 2
        windows = [{'id': 1, 'pid': 1, 'class': 'steam', 'app_id': None,
                    'app': None, 'workspace': 'store-steam', 'focused': not playing}]
        processes = []
        if playing:
            processes.append({'pid': 7, 'start': 70, 'ppid': 1, 'state': 'S', 'app': APP})
            windows.append({'id': 2, 'pid': 7, 'class': 'Generic game class', 'app_id': None,
                            'app': None, 'workspace': 'store-steam', 'focused': True})
        return processes, windows, 0


class LaunchTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name); self.base.chmod(0o700)
        self.driver = Driver(); self.clock = Clock()
        self.events = []
        self.stack = contextlib.ExitStack(); self.addCleanup(self.stack.close)
        def use(target, **kwargs): return self.stack.enter_context(patch(target, **kwargs))
        use('os.environ', new={**os.environ, 'XDG_RUNTIME_DIR': str(self.base)})
        self.stack.enter_context(patch.object(integration.session, 'Driver', return_value=self.driver))
        self.stack.enter_context(patch.object(integration.session, 'preflight'))
        self.stack.enter_context(patch.object(integration, 'check_game', return_value=(self.base, {'name': 'A new discovered game'})))
        self.stack.enter_context(patch.object(integration.time, 'monotonic', side_effect=self.clock.monotonic))
        self.stack.enter_context(patch.object(integration.time, 'sleep', side_effect=self.clock.sleep))
        self.stack.enter_context(patch.object(integration, 'event', side_effect=lambda *a: self.events.append(a)))
        self.stack.enter_context(patch.object(integration, 'client_stopped', return_value=True))
        self.stack.enter_context(patch.object(integration, 'live_identities', return_value=[]))
        def row(pid):
            if pid == Sender.pid:
                return {'pid': pid, 'start': 999, 'state': 'S'}
            raise FileNotFoundError()
        self.stack.enter_context(patch.object(integration.session, 'process_row', side_effect=row))
        def spawn(*args, **kwargs):
            self.driver.sent = True
            return Sender()
        self.popen = self.stack.enter_context(patch.object(integration.subprocess, 'Popen', side_effect=spawn))

    def test_generic_app_reaches_normal_return_without_stopping_client(self):
        integration.play(APP)
        self.assertEqual(self.driver.actions, [('launch', 0), ('home', 9)])
        record = integration.read_status()
        self.assertEqual(record['phase'], 'Completed')
        self.assertTrue(record['steam_still_running'])
        self.assertEqual(record['app'], APP)
        self.assertEqual(self.popen.call_args.args[0][-1], APP)

    def test_client_replacement_does_not_send_home(self):
        self.driver.broken = True
        integration.play(APP)
        self.assertEqual(self.driver.actions, [('launch', 0)])
        self.assertEqual(integration.read_status()['phase'], 'Failed')
        self.assertEqual(len(self.events), 1)

    def test_sender_failure_keeps_recovery_policy_and_no_home(self):
        with patch.object(Sender, 'returncode', 1):
            integration.play(APP)
        self.assertEqual(self.driver.actions, [('launch', 0)])
        self.assertEqual(integration.read_status()['phase'], 'Failed')

    def test_unresolved_dispatch_gap_is_not_recovered_from_absence(self):
        integration.write_status('Dispatching', dispatch_pending=True)
        with self.assertRaisesRegex(RuntimeError, 'recovery'):
            integration.play(APP)
        self.popen.assert_not_called()

    def test_stale_active_game_cannot_be_replaced(self):
        integration.write_status('Running', app=APP)
        with patch.object(integration, 'client_stopped', return_value=False):
            with self.assertRaisesRegex(RuntimeError, 'recovery'):
                integration.play(APP)
        self.popen.assert_not_called()

    def test_launcher_uses_unique_observer_and_only_selected_environment(self):
        with patch.object(integration.subprocess, 'run') as run, patch.dict(os.environ, {'PRIVATE_TEST_VALUE': 'secret', 'DISPLAY': ':1'}):
            run.return_value.returncode = 0
            integration.launch(APP)
        command = run.call_args.args[0]
        self.assertIn('--unit=zaman-steam-launch.service', command)
        self.assertIn('--wait', command)
        self.assertIn('--setenv=DISPLAY=:1', command)
        self.assertFalse(any('PRIVATE_TEST_VALUE' in value for value in command))
        self.assertEqual(command[-2:], ['_play', APP])

    def test_no_unvalidated_appid_reaches_systemd(self):
        with self.assertRaises(ValueError):
            integration.launch('1;anything')


if __name__ == '__main__':
    unittest.main()
