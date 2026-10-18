#!/usr/bin/env python3
"""Pure lifecycle checks; never starts Steam, Sway, D-Bus or systemd."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('probe', Path(__file__).with_name('zaman_steam_session.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def process(pid=10, start=100, ppid=1, app=None, state='S'):
    return dict(pid=pid, start=start, ppid=ppid, app=app, state=state)


def window(pid=10, app='1030300'):
    return dict(pid=pid, id=20, app=app, focused=True,
                workspace='store-steam', app_id=None, **{'class': f'steam_app_{app}'})


class Lifecycle(unittest.TestCase):
    def setUp(self):
        self.e = probe.Evidence('1030300')

    def sample(self, ps=(), ws=(), now=0, game=False, visible=True, menu=False):
        return self.e.sample(list(ps), list(ws), game, visible, menu, now)

    def begin(self):
        self.sample([process()], [window()])

    def test_generic_window_matches_exact_app_process(self):
        view = window(app=None)
        view.update(app_id='silksong', **{'class': None})
        state = self.sample([process(app='1030300')], [view])
        self.assertTrue(state['seen_window'])
        self.assertEqual(state['game_window_ids'], [20])
        self.sample(now=10)
        self.assertTrue(self.sample(now=13)['exit_evidence'])

    def test_generic_descendant_window_survives_parent_exit(self):
        view = window(pid=11, app=None)
        self.sample([process(app='1030300'), process(11, 101, 10)], [view])
        state = self.sample([process(11, 101, 1)], [view], now=10)
        self.assertTrue(state['seen_window'])
        self.assertEqual(state['alive'], [11])
        self.assertFalse(state['exit_evidence'])
        self.sample(now=20)
        self.assertTrue(self.sample(now=23)['exit_evidence'])

    def test_unrelated_client_window_is_not_a_game_window(self):
        state = self.sample([process(app='1030300'), process(20, 200)],
                            [window(pid=20, app=None)])
        self.assertFalse(state['seen_window'])
        self.assertEqual(state['game_window_ids'], [])
        self.sample(now=10)
        self.assertFalse(self.sample(now=20)['exit_evidence'])

    def test_reused_pid_generic_window_does_not_inherit_game_identity(self):
        self.sample([process(app='1030300')])
        state = self.sample([process(start=999)], [window(app=None)], now=10)
        self.assertFalse(state['seen_window'])
        self.assertEqual(state['game_window_ids'], [])

    def test_focus_or_window_loss_does_not_end_a_live_process(self):
        self.begin()
        self.assertFalse(self.sample([process()], now=20)['exit_evidence'])

    def test_no_game_window_is_never_success(self):
        self.sample([process(app='1030300')], now=0)
        self.sample(now=10)
        self.assertFalse(self.sample(now=20)['exit_evidence'])

    def test_exit_requires_stable_three_seconds(self):
        self.begin()
        self.assertFalse(self.sample(now=10)['exit_evidence'])
        self.assertFalse(self.sample(now=12.99)['exit_evidence'])
        self.assertTrue(self.sample(now=13)['exit_evidence'])

    def test_replacement_window_resets_exit_timer(self):
        self.begin()
        self.sample(now=10)
        self.sample([process(11, 110)], [window(11)], now=12)
        self.sample(now=15)
        self.assertFalse(self.sample(now=16)['exit_evidence'])
        self.assertTrue(self.sample(now=18)['exit_evidence'])

    def test_descendant_survives_parent_exit_and_reparenting(self):
        self.begin()
        self.sample([process(), process(11, 101, 10)], [window()], now=1)
        self.assertFalse(self.sample([process(11, 101, 1)], now=10)['exit_evidence'])
        self.sample(now=12)
        self.assertTrue(self.sample(now=15)['exit_evidence'])

    def test_pid_reuse_does_not_keep_dead_game_alive_or_claim_new_children(self):
        self.begin()
        reused = process(10, 999)
        self.sample([reused, process(12, 1000, 10)], now=10)
        self.assertTrue(self.sample([reused], now=13)['exit_evidence'])
        self.assertNotIn((12, 1000), self.e.known)

    def test_unrelated_game_cancels_return(self):
        self.begin()
        with self.assertRaisesRegex(RuntimeError, 'Another Steam App ID'):
            self.sample([process(11, 101, app='367520')], now=10)

    def test_daemon_game_guard_blocks_return(self):
        self.begin()
        self.sample(now=10, game=True)
        self.assertFalse(self.sample(now=20, game=True)['exit_evidence'])

    def test_menu_and_missing_client_each_reset_return_timer(self):
        for condition in ({'menu': True}, {'visible': False}):
            self.e = probe.Evidence('1030300')
            self.begin()
            self.sample(now=10)
            self.sample(now=12, **condition)
            self.assertFalse(self.sample(now=14)['exit_evidence'])
            self.assertTrue(self.sample(now=17)['exit_evidence'])

    def test_zombie_with_dead_window_is_not_running(self):
        self.begin()
        self.sample([process(state='Z')], now=10)
        self.assertTrue(self.sample([process(state='Z')], now=13)['exit_evidence'])


class Admission(unittest.TestCase):
    class Driver:
        def __init__(self, phase):
            self.phase = phase

        def call(self, method):
            return {
                'Status': ('Idle', '', '', '', '', ''),
                'InstalledGameStatus': ('0', '', '', 'Idle', '', False),
                'MenuStatus': (False, 0, ''),
                'MenuContext': (False, 0, '', 'idle', False, '', []),
                'TransferContext': (0, self.phase),
            }[method]

        def store(self):
            return ('', 0, 'Idle', 'zaman', False, '')

    def test_actual_lowercase_idle_contract_is_admitted(self):
        with mock.patch.object(probe.Path, 'iterdir', return_value=[]):
            probe.preflight(self.Driver('idle'))

    def test_completed_failed_transfer_is_admitted(self):
        with mock.patch.object(probe.Path, 'iterdir', return_value=[]):
            probe.preflight(self.Driver('failed'))

    def test_active_transfer_phases_remain_blocked(self):
        for phase in ('starting', 'active', 'stopping'):
            with self.subTest(phase=phase), self.assertRaisesRegex(RuntimeError, 'Close Transfer'):
                probe.preflight(self.Driver(phase))

    def test_unknown_and_wrong_case_phases_remain_blocked(self):
        for phase in ('Idle', 'unknown', 'CleanupFailed'):
            with self.subTest(phase=phase), self.assertRaisesRegex(RuntimeError, 'Close Transfer'):
                probe.preflight(self.Driver(phase))


class InputsAndOwnership(unittest.TestCase):
    def test_strict_app_ids(self):
        for value in ('0', '01', '-1', '123;touch /tmp/x', '1\n', '4294967296'):
            with self.assertRaises(ValueError):
                probe.valid_app(value)
        self.assertEqual(probe.valid_app('1030300'), '1030300')

    def test_environment_is_exact_key(self):
        self.assertIsNone(probe.env_app(b'OtherSteamAppId=123\0SteamAppId=0\0'))
        self.assertEqual(probe.env_app(b'TOKEN=not-retained\0SteamAppId=1030300\0'), '1030300')

    def test_scope_prefix_boundary(self):
        self.assertTrue(probe.in_group('/owned/a', '/owned'))
        self.assertFalse(probe.in_group('/owned-other', '/owned'))
        self.assertFalse(probe.in_group('/owned', '/'))

    def test_process_comm_can_contain_parentheses(self):
        fields = ['S', '7'] + ['0'] * 17 + ['12345'] + ['0'] * 4
        row = probe.parse_stat('10 (game (worker)) ' + ' '.join(fields))
        self.assertEqual((row['pid'], row['ppid'], row['start']), (10, 7, 12345))

    def test_window_traversal_includes_floating_native_and_xwayland(self):
        tree = {'type': 'workspace', 'name': 'store-steam', 'floating_nodes': [
            {'id': 1, 'pid': 10, 'app_id': 'steam_app_1030300'},
            {'id': 2, 'pid': 11, 'window_properties': {'class': 'steam_app_367520'}}]}
        rows = probe.window_rows(tree)
        self.assertEqual([r['app'] for r in rows], ['1030300', '367520'])
        self.assertTrue(all(r['workspace'] == 'store-steam' for r in rows))

    def test_dispatch_uses_native_binary_and_structured_app_argument(self):
        argv = probe.native_command(Path('/path with spaces/Steam'), '1030300')
        self.assertEqual(argv[-3:], ['-noverifyfiles', '-applaunch', '1030300'])
        self.assertEqual(argv[-4], '/path with spaces/Steam/steamrtarm64/steam')

    def test_duplicate_probe_cannot_acquire_transaction_lock(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'transaction.lock'
            fd = probe.acquire_lock(path)
            try:
                code = ('import fcntl, os, sys; '
                        'fd=os.open(sys.argv[1],os.O_RDWR); '
                        'fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)')
                result = subprocess.run([sys.executable, '-c', code, str(path)],
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('BlockingIOError', result.stderr)
            finally:
                os.close(fd)


if __name__ == '__main__':
    unittest.main()
