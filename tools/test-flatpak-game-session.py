"""Owned-instance tests with real child processes and fake external services."""
import importlib.util
import contextlib
import fcntl
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('flatpak-game-session.py')
spec = importlib.util.spec_from_file_location('session', SCRIPT)
session = importlib.util.module_from_spec(spec)
spec.loader.exec_module(session)
APP = 'org.supertuxproject.SuperTux'


class IdentityTests(unittest.TestCase):
    def test_instances_skip_headers_and_preserve_numeric_strings(self):
        rows = session.instances(f'Instance PID Application\n3126823604 10381 {APP}\n')
        self.assertEqual(rows, [{'instance': '3126823604', 'pid': 10381, 'app': APP}])

    def test_instance_mismatch_and_pid_replacement_never_select_an_app(self):
        rows = [{'instance': '123', 'pid': 10, 'app': APP}]
        self.assertEqual(session.owned_row(rows, APP, '123', 10), rows[0])
        self.assertIsNone(session.owned_row(rows, APP, '999', 10))
        for app, instance, pid in [('other.App', '123', 10), (APP, '123', 11), (APP, '--all', 10)]:
            with self.assertRaises(RuntimeError):
                session.owned_row(rows, app, instance, pid)

    def test_window_ownership_uses_exact_cgroup_or_descendants(self):
        group = '/app.slice/app-flatpak-owned.scope'
        tree = {'nodes': [{'type': 'workspace', 'name': 'library', 'nodes': [
            {'id': 1, 'pid': 10, 'name': 'Pegasus'},
            {'id': 2, 'pid': 20, 'name': 'Game'}], 'floating_nodes': [
            {'id': 3, 'pid': 30, 'name': 'Game dialog'},
            {'id': 4, 'pid': 40, 'name': 'Unrelated'}]}]}
        values = {10: '/session.scope', 20: group, 30: group + '/child', 40: group + '-other'}
        rows = session.window_rows(tree, group, values.__getitem__)
        self.assertEqual([row['id'] for row in rows], [2, 3])
        self.assertTrue(all(row['workspace'] == 'library' for row in rows))

    def test_force_kills_exact_instance_and_refuses_other_app(self):
        driver = session.Driver()
        record = {'app': APP, 'instance': '123', 'instance_pid': 20, 'instance_start': '99'}
        with patch.object(driver, 'rows', return_value=[{'instance': '123', 'pid': 20, 'app': APP}]), \
             patch.object(session, 'process_start', return_value='99'), patch.object(session, 'call') as call:
            driver.force(record)
            call.assert_called_once_with(['/usr/bin/flatpak', 'kill', '123'])
        with patch.object(driver, 'rows', return_value=[{'instance': '123', 'pid': 20, 'app': 'other.App'}]), \
             patch.object(session, 'call') as call:
            with self.assertRaises(RuntimeError):
                driver.force(record)
            call.assert_not_called()

    def test_sway_failure_is_not_accepted(self):
        with patch.object(session, 'call', return_value='[{"success":false,"error":"no matching node"}]'):
            with self.assertRaisesRegex(RuntimeError, 'Sway rejected'):
                session.Driver().sway('[con_id=2] fullscreen enable')

    def test_ref_requires_user_arm64_stable_and_no_external_copy(self):
        driver = session.Driver()
        with patch.object(session, 'call', return_value=f'app/{APP}/x86_64/stable'), \
             patch.object(driver, 'rows', return_value=[]):
            with self.assertRaisesRegex(RuntimeError, 'aarch64/stable'):
                driver.check_install(APP)
        with patch.object(session, 'call', return_value=f'app/{APP}/aarch64/stable'), \
             patch.object(driver, 'rows', return_value=[{'instance': '123', 'pid': 20, 'app': APP}]):
            with self.assertRaisesRegex(RuntimeError, 'Quit the existing'):
                driver.check_install(APP)

    def test_instance_fd_does_not_wait_for_long_lived_child(self):
        for payload in ('12345', '12345\\n', '12345\\0'):
            read_fd, write_fd = os.pipe()
            child = subprocess.Popen([sys.executable, '-c',
                'import os,time; os.write(int(__import__("sys").argv[1]), ' + repr(payload.encode().decode('unicode_escape').encode()) + '); time.sleep(30)',
                str(write_fd)], pass_fds=(write_fd,))
            os.close(write_fd)
            try:
                self.assertEqual(session.read_instance(read_fd, child, timeout=1), '12345')
                self.assertIsNone(child.poll())
            finally:
                os.close(read_fd)
                child.kill()
                child.wait()

    def test_invalid_instance_fd_is_refused(self):
        read_fd, write_fd = os.pipe()
        child = subprocess.Popen([sys.executable, '-c', 'import os; os.write(int(__import__("sys").argv[1]), b"not-an-id\\n")', str(write_fd)], pass_fds=(write_fd,))
        os.close(write_fd)
        try:
            with self.assertRaisesRegex(RuntimeError, 'numeric instance'):
                session.read_instance(read_fd, child, timeout=1)
        finally:
            os.close(read_fd)
            child.wait()

    def test_atomic_state_and_symlink_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'state.json'
            session.atomic_json(path, {'nonce': 'one', 'phase': 'Running'})
            self.assertEqual(session.load_json(path)['nonce'], 'one')
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            link = Path(directory) / 'link'
            link.symlink_to(path)
            with self.assertRaisesRegex(RuntimeError, 'symlink'):
                session.load_json(link)

    def test_wait_identity_skips_zero_pid_and_waits_for_scope_migration(self):
        rows = lambda pid: [{'instance': '123', 'pid': pid, 'app': APP}]
        driver = session.Driver()
        child = unittest.mock.Mock()
        child.poll.return_value = None
        clock = [0]
        def pause(delay):
            clock[0] += delay
        expected = f'/app.slice/app-flatpak-{APP}-123.scope'
        with patch.object(driver, 'rows', side_effect=[rows(0), rows(10), rows(10), rows(20), rows(20)]), \
             patch.object(session, 'process_start', return_value='99') as start, \
             patch.object(session, 'cgroup', side_effect=['/launch.service', expected]):
            identity = session.wait_identity(driver, APP, '123', child, clock=lambda: clock[0], pause=pause)
            self.assertEqual(identity['instance_pid'], 20)
            self.assertEqual(identity['cgroup'], expected)
            self.assertTrue(all(call.args[0] != 0 for call in start.call_args_list))

    def test_zero_pid_timeout_never_commits_an_identity(self):
        driver = session.Driver()
        child = unittest.mock.Mock()
        child.poll.return_value = None
        clock = [0]
        def pause(delay):
            clock[0] += delay
        with patch.object(driver, 'rows', return_value=[{'instance': '123', 'pid': 0, 'app': APP}]), \
             patch.object(session, 'process_start') as start:
            with self.assertRaisesRegex(RuntimeError, 'startup deadline'):
                session.wait_identity(driver, APP, '123', child, timeout=0.3, clock=lambda: clock[0], pause=pause)
            start.assert_not_called()

    def test_incomplete_zero_pid_record_recovers_only_exact_app_scope(self):
        driver = session.Driver()
        record = {'app': APP, 'instance': '123', 'instance_pid': 0, 'phase': 'CleanupFailed'}
        rows = [{'instance': '123', 'pid': 12044, 'app': APP}]
        scope = f'/app.slice/app-flatpak-{APP}-123.scope'
        with patch.object(driver, 'rows', return_value=rows), \
             patch.object(session, 'cgroup', return_value=scope), patch.object(session, 'call') as call:
            driver.force(record)
            call.assert_called_once_with(['/usr/bin/flatpak', 'kill', '123'])
        with patch.object(driver, 'rows', return_value=rows), \
             patch.object(session, 'cgroup', return_value='/unrelated.scope'), patch.object(session, 'call') as call:
            with self.assertRaisesRegex(RuntimeError, 'unexpected live scope'):
                driver.force(record)
            call.assert_not_called()

    def test_crash_probe_verifies_supervisor_identity_before_killing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            session.atomic_json(root / 'session.json', {'app': APP, 'nonce': 'a'*32,
                'launcher_pid': 4242, 'launcher_start': '99', 'instance': '123'})
            with patch.object(session, 'import_session'), patch.object(session, 'runtime_root', return_value=root), \
                 patch.object(session, 'call', return_value='8888'), \
                 patch.object(session.os, 'kill') as kill:
                with self.assertRaisesRegex(RuntimeError, 'identity changed'):
                    session.crash_probe()
                kill.assert_not_called()
            with patch.object(session, 'import_session'), patch.object(session, 'runtime_root', return_value=root), \
                 patch.object(session, 'call', return_value='4242'), \
                 patch.object(session, 'process_start', return_value='99'), \
                 patch.object(session.os, 'kill') as kill, \
                 patch.object(session.Driver, 'owned', return_value=None), \
                 patch.object(session, 'active_state', return_value='inactive'), \
                 patch.object(session.Driver, 'library'), contextlib.redirect_stdout(io.StringIO()) as output:
                session.crash_probe()
                kill.assert_called_once_with(4242, signal.SIGKILL)
                self.assertEqual(json.loads(output.getvalue())['parent_death_check'], 'passed')


class LifecycleTests(unittest.TestCase):
    def test_managed_launch_refuses_active_transaction_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock = os.open(root / 'transaction.lock', os.O_CREAT | os.O_RDWR, 0o600)
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            driver = unittest.mock.Mock()
            try:
                with patch.object(session, 'runtime_root', return_value=root), \
                     contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(session.run_game(APP, 'a' * 32, driver, managed=True), 1)
                driver.check_install.assert_not_called()
                self.assertIn('already active', session.load_json(root / 'session.json')['error'])
            finally:
                os.close(lock)

    def exercise_runtime(self, kind):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            marker = root / 'quit'
            child_holder = []
            original_popen = subprocess.Popen
            nonce = 'a' * 32
            def popen(argv, pass_fds):
                self.assertIn('--die-with-parent', argv)
                self.assertEqual(argv[-1], APP)
                fd = pass_fds[0]
                code = ('import os,time,sys;from pathlib import Path;'
                        'os.write(int(sys.argv[1]),b"456");'
                        'p=Path(sys.argv[2]);' +
                        '\nwhile not p.exists(): time.sleep(0.005)\n')
                child = original_popen([sys.executable, '-c', code, str(fd), str(marker)], pass_fds=pass_fds)
                child_holder.append(child)
                return child
            test = self
            class FakeDriver:
                placed = 0
                queried = 0
                closes = 0
                forced = 0
                restored = 0
                def check_install(self, app):
                    test.assertEqual(app, APP)
                def rows(self):
                    self.queried += 1
                    child = child_holder[0]
                    if kind == 'pid0' and self.queried == 1:
                        return [{'instance': '456', 'pid': 0, 'app': APP}]
                    return [] if child.poll() is not None else [{'instance': '456', 'pid': child.pid, 'app': APP}]
                def owned(self, record):
                    return self.rows()[0] if self.rows() else None
                def place(self, record, previous):
                    self.placed += 1
                    if kind in ('graceful', 'forced'):
                        session.atomic_json(root / 'close.json', {'nonce': nonce})
                    elif kind == 'stale':
                        session.atomic_json(root / 'close.json', {'nonce': 'b' * 32})
                        if self.placed == 2:
                            marker.touch()
                    else:
                        marker.touch()
                    return 32, [{'id': 32, 'pid': child_holder[0].pid, 'app_id': 'supertux2', 'fullscreen_mode': 1}]
                def close_windows(self, record):
                    self.closes += 1
                    if kind == 'graceful':
                        marker.touch()
                def force(self, record):
                    if self.rows():
                        self.forced += 1
                        child_holder[0].kill()
                def library(self):
                    self.restored += 1
            driver = FakeDriver()
            try:
                with patch.object(session, 'runtime_root', return_value=root), \
                     patch.object(session.subprocess, 'Popen', side_effect=popen), \
                     patch.object(session, 'cgroup', return_value=f'/app.slice/app-flatpak-{APP}-456.scope'), \
                     patch.object(session, 'process_start', return_value='99'), \
                     patch.object(session, 'GRACE_SECONDS', 0.01), contextlib.redirect_stdout(io.StringIO()):
                    result = session.run_game(APP, nonce, driver)
                state = session.load_json(root / 'session.json')
                self.assertEqual(result, 0)
                self.assertEqual(state['phase'], 'Exited')
                self.assertEqual(state['instance'], '456')
                self.assertGreater(state['instance_pid'], 0)
                self.assertEqual(driver.restored, 1)
                self.assertIsNotNone(child_holder[0].poll())
                return driver, state
            finally:
                for child in child_holder:
                    if child.poll() is None:
                        child.kill()
                    child.wait()

    def test_startup_zero_pid_does_not_create_a_cleanup_failure(self):
        driver, state = self.exercise_runtime('pid0')
        self.assertEqual(driver.forced, 0)

    def test_natural_exit_releases_only_owned_session(self):
        driver, state = self.exercise_runtime('natural')
        self.assertEqual(driver.closes, 0)
        self.assertEqual(driver.forced, 0)

    def test_graceful_close_precedes_forced_cleanup(self):
        driver, state = self.exercise_runtime('graceful')
        self.assertEqual(driver.closes, 1)
        self.assertEqual(driver.forced, 0)

    def test_unresponsive_game_uses_owned_instance_fallback(self):
        driver, state = self.exercise_runtime('forced')
        self.assertEqual(driver.closes, 1)
        self.assertEqual(driver.forced, 1)
        self.assertTrue(state['forced'])

    def test_stale_close_request_is_ignored(self):
        driver, state = self.exercise_runtime('stale')
        self.assertGreaterEqual(driver.placed, 2)
        self.assertEqual(driver.closes, 0)
        self.assertEqual(driver.forced, 0)

    def test_owned_launcher_places_then_closes_without_touching_other_apps(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = {'app': APP, 'instance': '123', 'instance_pid': 20, 'instance_start': '99', 'cgroup': '/own.scope'}
            driver = session.Driver()
            before = {'nodes': [{'type': 'workspace', 'name': 'library', 'nodes': [
                {'id': 2, 'pid': 20, 'name': 'SuperTux', 'fullscreen_mode': 0},
                {'id': 1, 'pid': 10, 'name': 'Pegasus', 'fullscreen_mode': 0}]}]}
            after = {'nodes': [{'type': 'workspace', 'name': session.WORKSPACE, 'nodes': [
                {'id': 2, 'pid': 20, 'name': 'SuperTux', 'fullscreen_mode': 1}]}]}
            with patch.object(driver, 'owned', return_value={'pid': 20}), \
                 patch.object(driver, 'tree', side_effect=[before, after, after]), \
                 patch.object(session, 'cgroup', side_effect=lambda pid: '/own.scope' if pid == 20 else '/other.scope'), \
                 patch.object(driver, 'sway') as sway:
                # window_rows' injectable default is bound when defined; explicitly rebind it in this fixture.
                original = session.window_rows
                def rows(tree, group):
                    return original(tree, group, session.cgroup)
                with patch.object(session, 'window_rows', side_effect=rows):
                    primary, windows = driver.place(record, None)
                    self.assertEqual(primary, 2)
                    self.assertEqual(windows[0]['fullscreen_mode'], 1)
                    driver.close_windows(record)
                commands = [call.args[0] for call in sway.call_args_list]
                self.assertTrue(any('fullscreen enable' in command for command in commands))
                self.assertIn('[con_id=2] kill', commands)
                self.assertTrue(all('[con_id=1]' not in command for command in commands))


if __name__ == '__main__':
    unittest.main()
