#!/usr/bin/env python3
import importlib.util
import math
from pathlib import Path
import socket
import unittest

spec = importlib.util.spec_from_file_location('shortcut', Path(__file__).with_name('zaman-menu-shortcut.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ShortcutTests(unittest.TestCase):
    def test_chord_fires_once_until_release(self):
        guard = module.TriggerGuard()
        self.assertTrue(guard.accept('/a', module.EVENT, 1, 10))
        self.assertFalse(guard.accept('/a', module.EVENT, 1, 12))
        self.assertFalse(guard.accept('/a', module.EVENT, 0, 13))
        self.assertTrue(guard.accept('/a', module.EVENT, 1, 14))

    def test_duplicate_transports_do_not_launch_twice(self):
        guard = module.TriggerGuard()
        self.assertTrue(guard.accept('/usb', module.EVENT, 1, 10))
        self.assertFalse(guard.accept('/bt', module.EVENT, 1, 10.1))
        guard.accept('/usb', module.EVENT, 0, 10.2)
        guard.accept('/bt', module.EVENT, 0, 10.2)
        self.assertTrue(guard.accept('/bt', module.EVENT, 1, 12))

    def test_other_events_release_and_invalid_values_never_launch(self):
        for event, value in [('ui_guide', 1), ('ui_accept', 1), (module.EVENT, 0),
                (module.EVENT, math.nan), (module.EVENT, math.inf), (module.EVENT, '1')]:
            self.assertFalse(module.TriggerGuard().accept('/a', event, value, 10))

    def test_restart_clears_held_event_state(self):
        guard = module.TriggerGuard()
        guard.accept('/a', module.EVENT, 1, 10)
        guard.reset()
        self.assertTrue(guard.accept('/a', module.EVENT, 1, 12))

    def test_manual_preview_is_detected_by_its_real_lock(self):
        runtime = '/test-zaman-shortcut-' + str(__import__('os').getpid())
        try:
            self.assertFalse(module.preview_running(runtime))
        except PermissionError:
            self.skipTest('This execution environment blocks Unix sockets; run on the Dragon.')
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as lock:
            lock.bind('\0zaman-menu-dev:' + runtime)
            self.assertTrue(module.preview_running(runtime))
        self.assertFalse(module.preview_running(runtime))

if __name__ == '__main__':
    unittest.main()
