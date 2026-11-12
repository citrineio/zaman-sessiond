#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('model', Path(__file__).resolve().parents[1] / 'notifications/model.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ReceiptTests(unittest.TestCase):
    def test_lost_ack_restart_dismiss_and_clear_do_not_replay(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'history.json'
            model = m.Model(path)
            key = 'transaction:' + 'a' * 32 + ':1'
            model.event('Dukkan', key, 'Installed', target='dukkan-transaction:' + 'b' * 32)
            self.assertEqual(len(model.records), 1)
            self.assertEqual(model.records[0]['target'], 'dukkan-transaction:' + 'b' * 32)
            model = m.Model(path)
            model.event('Dukkan', key, 'Lost acknowledgement')
            self.assertEqual(len(model.records), 1)
            self.assertEqual(model.visible, [])
            model.command({'action': 'clear', 'ids': [model.records[0]['id']]})
            model = m.Model(path)
            model.event('Dukkan', key, 'Already cleared')
            self.assertEqual(model.records, [])
            model.event('Dukkan', key[:-1] + '2', 'Next transaction')
            self.assertEqual(len(model.records), 1)

    def test_out_of_order_requires_retry_and_never_advances_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            model = m.Model(Path(directory) / 'history.json')
            with self.assertRaises(ValueError):
                model.event('Dukkan', 'transaction:' + 'a' * 32 + ':2', 'Out of order')
            self.assertEqual(model.transaction_receipts, {})

    def test_persistence_failure_cannot_ack_or_leave_a_duplicate(self):
        with tempfile.TemporaryDirectory() as directory:
            model = m.Model(Path(directory) / 'history.json')
            key = 'transaction:' + 'a' * 32 + ':1'
            with patch.object(m.os, 'replace', side_effect=OSError('disk full')):
                with self.assertRaises(OSError):
                    model.event('Dukkan', key, 'Installed')
            self.assertEqual(model.records, [])
            self.assertEqual(model.transaction_receipts, {})
            model.event('Dukkan', key, 'Installed')
            self.assertEqual(len(model.records), 1)

    def test_dnd_retains_receipt_and_history_without_a_popup(self):
        with tempfile.TemporaryDirectory() as directory:
            model = m.Model(Path(directory) / 'history.json')
            model.command({'action': 'preferences', 'dnd': True})
            model.event('Dukkan', 'transaction:' + 'a' * 32 + ':1', 'Installed')
            self.assertEqual(model.visible, [])
            self.assertEqual(len(model.records), 1)

    def test_targets_are_typed_data(self):
        self.assertEqual(m.Model.target('dukkan-transaction:' + 'a' * 32), 'dukkan-transaction:' + 'a' * 32)
        for target in ['file:///tmp/run', 'dukkan-transaction:../../bad', 'steam:run', 'sh -c anything']:
            self.assertEqual(m.Model.target(target), '')


if __name__ == '__main__':
    unittest.main()
